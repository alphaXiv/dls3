//! Functions operating against the backing store

use rustix::{
    fs::{AtFlags, FallocateFlags, Mode, OFlags, ResolveFlags},
    io::Errno,
    path::Arg,
};
use snafu::ResultExt;
use snafu::prelude::Snafu;
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    io,
    os::fd::{AsFd, FromRawFd, IntoRawFd},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
    time::Instant,
};
use tokio::sync::broadcast;
use tracing::{error, trace};

use aws_sdk_s3::{
    Client,
    error::{ProvideErrorMetadata, SdkError},
    operation::list_objects_v2::ListObjectsV2Error,
};

use crate::config::Config;

#[derive(Debug)]
pub struct Store {
    client: Client,
    config: Config,
    root_fd: rustix::fd::OwnedFd,
    /// TODO: merge these hashmaps, maybe use the hashheap crate
    /// Files that at least one client is using
    opened_files: Mutex<HashMap<String, OpenedFileState>>,
    /// Files that no client is using but we have downloaded the contents of
    cached_files: Mutex<HashMap<String, CachedFileState>>,
}

#[derive(Debug)]
pub struct StoreHandle {
    store: Arc<Store>,
    opened_files: HashSet<String>,
}

#[derive(Debug)]
struct OpenedFileState {
    refs: u32,
    download_status: FileDownloadStatus,
}

#[derive(Debug, Eq, PartialEq)]
struct CachedFileState {
    last_access: Instant,
    size: u64,
    etag: Option<String>,
}

impl Ord for CachedFileState {
    fn cmp(&self, other: &Self) -> Ordering {
        // larger files are higher priority (evicted first)
        self.size
            .cmp(&other.size)
            // among files of the same size, less-recently-used ones are higher priority
            .then_with(|| other.last_access.cmp(&self.last_access))
    }
}

impl PartialOrd for CachedFileState {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug)]
enum FileDownloadStatus {
    Downloading {
        /// Channel through which to receive the outcome of the attempt to download the file
        /// once it completes
        download_result_channel: broadcast::Sender<Result<(), u16>>,
    },
    Downloaded {
        etag: Option<String>,
    },
}

#[derive(Debug, Snafu)]
pub enum InitError {
    #[snafu(display("Could not list objects in bucket {bucket}"))]
    S3 {
        source: Box<SdkError<ListObjectsV2Error>>,
        bucket: String,
    },
    #[snafu(display("Could not create {path} in backing store"))]
    Io {
        source: std::io::Error,
        path: String,
    },
    #[snafu(display("Could not create {path} in backing store"))]
    Rustix { source: Errno, path: String },
}

impl Store {
    /// Initialize a backing store with sparse files from the contents of an S3 bucket
    pub async fn new(client: &Client, config: Config) -> Result<Arc<Store>, InitError> {
        tokio::fs::create_dir_all(config.backing_path.as_ref())
            .await
            .with_context(|_| IoSnafu {
                path: config.backing_path.clone(),
            })?;

        let store = Arc::new(Store {
            client: client.clone(),
            root_fd: rustix::fs::open(
                config.backing_path.as_ref(),
                OFlags::DIRECTORY,
                Mode::empty(),
            )
            .with_context(|_| RustixSnafu {
                path: config.backing_path.clone(),
            })?,
            opened_files: Mutex::new(HashMap::new()),
            cached_files: Mutex::new(HashMap::new()),
            config,
        });
        let mut created_files: HashMap<String, bool> = HashMap::new();
        store.sync_backing_store(&mut created_files).await?;

        let weak = Arc::downgrade(&store);
        let interval = store.config.refetch_all_interval;
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(interval).await;
                let Some(store) = weak.upgrade() else {
                    return;
                };
                if let Err(err) = store.sync_backing_store(&mut created_files).await {
                    error!(?err, "failed to refetch files");
                }
            }
        });

        Ok(store)
    }

    pub fn handle(self: Arc<Store>) -> StoreHandle {
        StoreHandle {
            store: self,
            opened_files: HashSet::new(),
        }
    }

    /// Returns ETag or an error
    async fn download_file(self: &Store, path: &str) -> Result<Option<String>, io::Error> {
        // TODO: create enclosing directories if they are missing
        let fd = rustix::fs::openat2(
            self.root_fd.as_fd(),
            path,
            OFlags::CREATE | OFlags::WRONLY,
            Mode::from_bits(0o666).unwrap(),
            ResolveFlags::BENEATH,
        )?;

        let stat = rustix::fs::fstat(&fd)?;
        let len = if stat.st_size == 0 {
            // maybe just created
            let head = self
                .client
                .head_object()
                .key(path)
                .bucket(self.config.bucket.as_ref())
                .send()
                .await
                .map_err(|err| {
                    error!(?path, ?err, "failed to HEAD S3 object");
                    io::Error::from_raw_os_error(Errno::raw_os_error(
                        match err.into_service_error() {
                            aws_sdk_s3::operation::head_object::HeadObjectError::NotFound(_) => {
                                Errno::NOENT
                            }
                            whatever if whatever.code() == Some("AccessDenied") => Errno::ACCESS,
                            _ => Errno::IO,
                        },
                    ))
                })?;
            head.content_length().unwrap_or(0)
        } else {
            stat.st_size
        };

        // reserve space for the full size
        while let Err(err) = rustix::fs::fallocate(&fd, FallocateFlags::empty(), 0, len as u64) {
            if err == Errno::NOSPC {
                let maybe_evictable_entry = {
                    let mut guard = self.cached_files.lock().unwrap();
                    let maybe_entry = guard.iter().max_by_key(|(_key, cache_state)| *cache_state);
                    if let Some((key, _cache_state)) = maybe_entry {
                        // TODO: do something better
                        let owned_key = key.to_string();
                        Some(guard.remove_entry(&owned_key).unwrap())
                    } else {
                        None
                    }
                };
                if let Some(evictable_entry) = maybe_evictable_entry {
                    trace!(
                        key = evictable_entry.0,
                        size = evictable_entry.1.size,
                        "evicting"
                    );
                    let evict_fd = rustix::fs::openat2(
                        self.root_fd.as_fd(),
                        evictable_entry.0,
                        OFlags::WRONLY,
                        Mode::empty(),
                        ResolveFlags::BENEATH,
                    )?;
                    // clear space
                    rustix::fs::fallocate(
                        evict_fd,
                        FallocateFlags::PUNCH_HOLE | FallocateFlags::KEEP_SIZE,
                        0,
                        evictable_entry.1.size,
                    )?;
                    // retry
                } else {
                    return Err(io::Error::from(err));
                }
            } else {
                return Err(io::Error::from(err));
            }
        }

        let result = self
            .client
            .get_object()
            .key(path)
            .bucket(self.config.bucket.as_ref())
            .send()
            .await
            .map_err(|err| {
                error!(?path, ?err, "failed to download S3 object");
                io::Error::from_raw_os_error(Errno::raw_os_error(match err.into_service_error() {
                    aws_sdk_s3::operation::get_object::GetObjectError::NoSuchKey(_) => Errno::NOENT,
                    whatever if whatever.code() == Some("AccessDenied") => Errno::ACCESS,
                    _ => Errno::IO,
                }))
            })?;
        let _ = tokio::io::copy(&mut result.body.into_async_read(), &mut unsafe {
            tokio::fs::File::from_raw_fd(fd.into_raw_fd())
        })
        .await?;
        Ok(result.e_tag)
    }

    fn release_file<'a>(
        self: &'a Store,
        path: &str,
        opened_files_guard: Option<&mut MutexGuard<'a, HashMap<String, OpenedFileState>>>,
    ) {
        let maybe_owned_path_and_state = {
            let mut owned_guard: MutexGuard<HashMap<String, OpenedFileState>>;
            let guard = match opened_files_guard {
                Some(g) => g,
                None => {
                    owned_guard = self.opened_files.lock().unwrap();
                    &mut owned_guard
                }
            };
            if let Some(file) = guard.get_mut(path) {
                file.refs -= 1;
                if file.refs == 0 {
                    Some(guard.remove_entry(path).unwrap())
                } else {
                    None
                }
            } else {
                None
            }
        };

        if let Some((
            owned_path,
            OpenedFileState {
                refs: _,
                download_status: FileDownloadStatus::Downloaded { etag },
            },
        )) = maybe_owned_path_and_state
        {
            // if file was unlinked, we don't need to insert it into the cache
            let Ok(stat) = rustix::fs::statat(self.root_fd.as_fd(), path, AtFlags::empty()) else {
                return;
            };
            if stat.st_size == 0 {
                return;
            }
            trace!(path, "releasing file to cache");
            let mut guard = self.cached_files.lock().unwrap();
            guard.insert(
                owned_path,
                CachedFileState {
                    last_access: Instant::now(),
                    size: stat.st_size as u64,
                    etag,
                },
            );
        }
    }

    /// Make the contents of the backing store match the S3 bucket by allocating sparse
    /// `created_files` holds the keys of files already present in the backing store. Keys that are
    /// in `created_files` but not in S3 will have their backing files deleted.
    async fn sync_backing_store(
        self: &Store,
        created_files: &mut HashMap<String, bool>,
    ) -> Result<(), InitError> {
        let mut pages = self
            .client
            .list_objects_v2()
            .bucket(self.config.bucket.as_ref())
            .prefix(self.config.prefix.as_ref())
            .into_paginator()
            .send();
        let mut path = PathBuf::from(self.config.backing_path.as_ref());
        for visited in created_files.values_mut() {
            *visited = false;
        }

        let mut created: u64 = 0;
        let mut deleted: u64 = 0;

        while let Some(page_result) = pages.next().await {
            let page = page_result.map_err(Box::new).context(S3Snafu {
                bucket: self.config.bucket.as_ref(),
            })?;
            for object in page.contents() {
                if let Some(key) = object.key() {
                    // if it's in cached_files or opened_files, skip
                    // (will be handled by refetching *open* files)
                    // but mark the file as visited
                    if self.cached_files.lock().unwrap().contains_key(key)
                        || self.opened_files.lock().unwrap().contains_key(key)
                    {
                        *created_files.get_mut(key).unwrap() = true;
                        continue;
                    }

                    let object_path = Path::new(key);

                    if let (Some(parent), Some(file_name)) =
                        (object_path.parent(), object_path.file_name())
                    {
                        path.push(parent);
                        tokio::fs::create_dir_all(&path)
                            .await
                            .with_context(|_| IoSnafu {
                                path: path.to_string_lossy(),
                            })?;
                        path.push(file_name);
                        let file =
                            tokio::fs::File::create(&path)
                                .await
                                .with_context(|_| IoSnafu {
                                    path: path.to_string_lossy(),
                                })?;
                        rustix::fs::ftruncate(file.as_fd(), object.size().unwrap_or(0) as u64)
                            .with_context(|_| RustixSnafu {
                                path: path.to_string_lossy(),
                            })?;

                        if let Some(existing_val_ref) = created_files.get_mut(key) {
                            *existing_val_ref = true;
                        } else {
                            created += 1;
                            created_files.insert(key.to_string(), true);
                        }
                    }
                }
                path.push(self.config.backing_path.as_ref());
            }
        }

        // make sure we don't try to use these files again
        {
            let mut guard = self.cached_files.lock().unwrap();
            for (key, _visited) in created_files.iter().filter(|(_key, visited)| !*visited) {
                guard.remove(key);
            }
        }
        // now delete them
        for (key, _visited) in created_files.iter().filter(|(_key, visited)| !*visited) {
            rustix::fs::unlinkat(self.root_fd.as_fd(), key, AtFlags::empty())
                .with_context(|_| RustixSnafu { path: key })?;
            deleted += 1;
        }
        // clean up keys
        created_files.retain(|_key, visited| *visited);

        trace!("sync: created {created} files, deleted {deleted}");

        Ok(())
    }
}

impl StoreHandle {
    /// Error is an errno value.
    pub async fn open_file(&mut self, path: &str) -> Result<(), u16> {
        if let Some((owned_path, state)) =
            self.store.cached_files.lock().unwrap().remove_entry(path)
        {
            trace!(path, "reviving cached entry");
            self.store
                .opened_files
                .lock()
                .unwrap()
                .entry(owned_path)
                .or_insert(OpenedFileState {
                    refs: 0,
                    download_status: FileDownloadStatus::Downloaded { etag: state.etag },
                })
                .refs += 1;
            return Ok(());
        }

        let (tx_download, mut rx_download) = {
            let mut guard = self.store.opened_files.lock().unwrap();
            if let Some(existing_state) = guard.get_mut(path) {
                existing_state.refs += 1;
                let rx_download = match &existing_state.download_status {
                    FileDownloadStatus::Downloaded { .. } => None,
                    FileDownloadStatus::Downloading {
                        download_result_channel,
                    } => Some(download_result_channel.subscribe()),
                };
                (None, rx_download)
            } else {
                let (tx, _) = broadcast::channel::<Result<(), u16>>(1);

                let state = OpenedFileState {
                    refs: 1,
                    download_status: FileDownloadStatus::Downloading {
                        download_result_channel: tx.clone(),
                    },
                };
                guard.insert(path.to_string(), state);
                (Some(tx), None)
            }
        };

        let result = if let Some(ref mut rx) = rx_download {
            rx.recv().await.unwrap().map(|_| {}) // TODO investigate
        } else if let Some(ref tx) = tx_download {
            let result = self.store.download_file(path).await.map_err(|err| {
                err.raw_os_error()
                    .unwrap_or(rustix::io::Errno::IO.raw_os_error()) as u16
            });
            let _ = tx.send(match &result {
                Ok(_) => Ok(()),
                Err(err) => Err(*err),
            });
            let mut guard = self.store.opened_files.lock().unwrap();
            match result {
                Ok(etag) => {
                    if let Some(state) = guard.get_mut(path) {
                        state.download_status = FileDownloadStatus::Downloaded { etag };
                    }
                    Ok(())
                }
                Err(err) => {
                    guard.remove(path);
                    Err(err)
                }
            }
        } else {
            Ok(())
        };

        trace!(
            mode = match (tx_download, rx_download) {
                (_, Some(_)) => "follow simultaneous download",
                (Some(_), _) => "download ourselves",
                _ => "already downloaded",
            },
            path,
            errno = result.err(),
            "opened file"
        );

        if result.is_ok() {
            self.opened_files.insert(path.to_string());
        }

        result
    }

    pub fn close_file(&mut self, path: &str) {
        self.store.release_file(path, None);
        self.opened_files.remove(path);
    }
}

impl Drop for StoreHandle {
    fn drop(&mut self) {
        trace!(
            "cleaning {} files left open by client",
            self.opened_files.len()
        );
        if !self.opened_files.is_empty() {
            let mut guard = self.store.opened_files.lock().unwrap();
            for key in &self.opened_files {
                self.store.release_file(key, Some(&mut guard));
            }
        }
    }
}
