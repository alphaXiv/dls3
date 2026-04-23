//! Functions operating against the backing store

use rustix::{
    fs::{FallocateFlags, Mode, OFlags, ResolveFlags},
    io::Errno,
    path::Arg,
};
use snafu::ResultExt;
use snafu::prelude::Snafu;
use std::{
    collections::{HashMap, HashSet},
    io,
    os::fd::{AsFd, FromRawFd, IntoRawFd},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::sync::broadcast;
use tracing::{error, trace};

use aws_sdk_s3::{Client, error::SdkError, operation::list_objects_v2::ListObjectsV2Error};

use crate::config::Config;

#[derive(Debug)]
pub struct Store {
    client: Client,
    config: Config,
    root_path: String,
    root_fd: rustix::fd::OwnedFd,
    present_files: Mutex<HashMap<String, FileState>>,
}

#[derive(Debug)]
pub struct StoreHandle {
    store: Arc<Store>,
    opened_files: HashSet<String>,
}

#[derive(Debug)]
struct FileState {
    refs: u32,
    download_status: FileDownloadStatus,
}

#[derive(Debug)]
enum FileDownloadStatus {
    Downloading {
        /// Channel through which to receive the outcome of the attempt to download the file
        /// once it completes
        download_result_channel: broadcast::Sender<Result<(), i32>>,
    },
    Downloaded,
}

#[derive(Debug, Snafu)]
pub enum InitError {
    #[snafu(display("Could not list objects in bucket {bucket}"))]
    S3 {
        source: SdkError<ListObjectsV2Error>,
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
        let mut pages = client
            .list_objects_v2()
            .bucket(config.bucket.as_ref())
            .prefix(config.prefix.as_ref())
            .into_paginator()
            .send();

        let mut path = PathBuf::from(config.backing_path.as_ref());
        while let Some(page_result) = pages.next().await {
            let page = page_result.context(S3Snafu {
                bucket: config.bucket.as_ref(),
            })?;
            for object in page.contents() {
                if let Some(key) = object.key() {
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
                    }
                }
                path.push(config.backing_path.as_ref());
            }
        }

        Ok(Arc::new(Store {
            client: client.clone(),
            root_path: config.backing_path.to_string(),
            root_fd: rustix::fs::open(
                config.backing_path.as_ref(),
                OFlags::DIRECTORY,
                Mode::empty(),
            )
            .with_context(|_| RustixSnafu {
                path: path.to_string_lossy(),
            })?,
            present_files: Mutex::new(HashMap::new()),
            config,
        }))
    }

    pub fn handle(self: Arc<Store>) -> StoreHandle {
        StoreHandle {
            store: self,
            opened_files: HashSet::new(),
        }
    }

    async fn download_file(self: &Store, path: &str) -> Result<(), io::Error> {
        // TODO: create enclosing directories if they are missing
        let fd = rustix::fs::openat2(
            self.root_fd.as_fd(),
            path,
            OFlags::CREATE | OFlags::WRONLY,
            Mode::RUSR | Mode::WUSR | Mode::RGRP | Mode::WGRP | Mode::ROTH | Mode::WOTH,
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
                    io::Error::from_raw_os_error(rustix::io::Errno::IO.raw_os_error())
                })?;
            head.content_length().unwrap_or(0)
        } else {
            stat.st_size
        };

        // reserve space for the full size
        rustix::fs::fallocate(&fd, FallocateFlags::empty(), 0, len as u64)?;

        let result = self
            .client
            .get_object()
            .key(path)
            .bucket(self.config.bucket.as_ref())
            .send()
            .await
            .map_err(|err| {
                error!(?path, ?err, "failed to download S3 object");
                io::Error::from_raw_os_error(rustix::io::Errno::IO.raw_os_error())
            })?;
        let _ = tokio::io::copy(&mut result.body.into_async_read(), &mut unsafe {
            tokio::fs::File::from_raw_fd(fd.into_raw_fd())
        })
        .await?;
        Ok(())
    }
}

impl StoreHandle {
    /// Error is an errno value.
    pub async fn open_file(&mut self, path: &str) -> Result<(), i32> {
        let (tx_download, mut rx_download) = {
            let mut guard = self.store.present_files.lock().unwrap();
            if let Some(existing_state) = guard.get_mut(path) {
                existing_state.refs += 1;
                let rx_download = match &existing_state.download_status {
                    FileDownloadStatus::Downloaded => None,
                    FileDownloadStatus::Downloading {
                        download_result_channel,
                    } => Some(download_result_channel.subscribe()),
                };
                (None, rx_download)
            } else {
                let (tx, _) = broadcast::channel::<Result<(), i32>>(1);

                let state = FileState {
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
            rx.recv().await.unwrap()
        } else if let Some(ref tx) = tx_download {
            let result = self.store.download_file(path).await.map_err(|err| {
                err.raw_os_error()
                    .unwrap_or(rustix::io::Errno::IO.raw_os_error())
            });
            let _ = tx.send(result);
            let mut guard = self.store.present_files.lock().unwrap();
            match result {
                Ok(_) => {
                    if let Some(state) = guard.get_mut(path) {
                        state.download_status = FileDownloadStatus::Downloaded;
                    }
                }
                Err(_) => {
                    guard.remove(path);
                }
            };
            result
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
}
