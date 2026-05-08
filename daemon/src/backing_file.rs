use rustix::fs::{self, FallocateFlags, Mode, OFlags, ResolveFlags};
use std::{
    io,
    os::fd::OwnedFd,
    sync::{Arc, Mutex, Weak},
};
use tokio::sync::broadcast::{self, error::RecvError};
use tracing::error;

use crate::store::Store;

/// Tracks the state of an on-disk file allocated for an S3 object. Only deleted when the S3 object
/// goes away.
#[derive(Debug)]
pub struct File {
    key: String,
    store: Arc<Store>,
    state: Mutex<FileState>,
}

#[derive(Debug)]
struct FileState {
    length: u64,
    status: FileDownloadStatus,
}

#[derive(Debug)]
enum FileDownloadStatus {
    Sparse,
    Downloading {
        /// Channel to receive updates on the download progress. Each value sent is the total number
        /// of bytes that have been written to the file so far.
        /// TOOD: might be better to use an atomic int or cvar or something here
        tx: broadcast::Sender<u64>,
        ref_count: u32,
    },
    Complete {
        // TODO: include an ETag
        ref_count: u32,
    },
    Cached, // TODO: include an ETag
}

/// Reference to a file that a victim has open. One of these is created for each open() call and
/// destroyed for the close().
///
/// It has a weak reference to the file so that it can find out if the file was deleted from S3.
#[derive(Debug)]
pub struct FileHandle {
    file: Weak<File>,
}

impl File {
    pub fn init_sparse(
        key: String,
        length: u64,
        store: Arc<Store>,
    ) -> Result<Arc<File>, io::Error> {
        let file = File {
            key,
            store,
            state: Mutex::new(FileState {
                length,
                status: FileDownloadStatus::Sparse,
            }),
        };
        let fd = file.fd(OFlags::WRONLY | OFlags::CREATE | OFlags::TRUNC)?;
        fs::ftruncate(fd, length)?;
        Ok(Arc::new(file))
    }

    /// Ensure physical disk space is allocated to this file and return a handle.
    /// If ENOSPC, caller deletes a cached file and tries again.
    pub fn open(self: &Arc<File>) -> Result<FileHandle, io::Error> {
        let mut guard = self.state.lock().unwrap();
        match guard.status {
            FileDownloadStatus::Sparse => {
                let fd = self.fd(OFlags::WRONLY)?;
                fs::fallocate(
                    fd,
                    FallocateFlags::PUNCH_HOLE | FallocateFlags::KEEP_SIZE,
                    0,
                    guard.length,
                )?;
                guard.status = FileDownloadStatus::Downloading {
                    tx: todo!(), // spawn a task to download the file
                    ref_count: 1,
                }
            }
            FileDownloadStatus::Complete { ref mut ref_count }
            | FileDownloadStatus::Downloading {
                tx: _,
                ref mut ref_count,
            } => *ref_count += 1,
            FileDownloadStatus::Cached => {
                guard.status = FileDownloadStatus::Complete { ref_count: 1 }
            }
        }
        Ok(FileHandle {
            file: Arc::downgrade(self),
        })
    }

    /// Compare this file against the S3 state:
    /// - If the file is already being downloaded, do nothing
    /// - If the file is fully downloaded, and the S3 version is different, start downloading the
    ///   new version. Future reads will block until that part of the new file version is downloaded.
    /// - If the file is cached, and the S3 version is different, deallocate disk space and update the length
    /// - If the file is sparse, and the S3 version is different, update the length
    /// - If the file no longer exists, unlink it and return None. Caller should remove the file
    ///   from its tracking. Existing file descriptors can read the part that was partially downloaded.
    ///
    /// The idea here is that when syncing, the store will take the file out of its hashmap, call this,
    /// and then only reinsert the file if this returns Some.
    pub async fn sync(self: Arc<File>) -> Option<Arc<File>> {
        let guard = self.state.lock().unwrap();
        match guard.status {
            FileDownloadStatus::Sparse => {
                // HEAD request to see if the length or timestamp has changed
                // or if the file was deleted, return None
                todo!()
            }
            FileDownloadStatus::Downloading { .. } => {
                // don't need to do anything
            }
            FileDownloadStatus::Complete { ref_count } => {
                // GET request with If-None-Match to see if the file has changed from the version
                // we have an ETag for

                // if so, move to Downloading state. future reads will block until we have downloaded
                // up to that point in the new version of the file
                todo!()
            }
            FileDownloadStatus::Cached => {
                // HEAD request with If-None-Match to see if there is a new version of the file.
                // if so, use fallocate with PUNCH_HOLE | KEEP_SIZE to free the disk space
                // and return to the Sparse state.
                // otherwise leave it
                todo!()
            }
        }
    }

    /// Called by FileHandle::drop
    fn unref(&self) {
        let mut guard = self.state.lock().unwrap();
        match guard.status {
            FileDownloadStatus::Sparse | FileDownloadStatus::Cached => panic!(
                "handle existed for file {} in state {:?}",
                &self.key, *guard
            ),
            FileDownloadStatus::Downloading {
                tx: _,
                ref mut ref_count,
            } => {
                *ref_count -= 1;
                if *ref_count == 0 {
                    // TODO: abort the download
                    guard.status = FileDownloadStatus::Sparse;
                    let fd = match self.fd(OFlags::WRONLY) {
                        Ok(fd) => fd,
                        Err(err) => {
                            return tracing::error!(
                                ?err,
                                "failed opening {} to truncate",
                                self.key
                            );
                        }
                    };
                    match fs::ftruncate(fd, guard.length) {
                        Ok(()) => {}
                        Err(err) => tracing::error!(?err, "failed truncating {}", self.key),
                    }
                }
            }
            FileDownloadStatus::Complete { ref mut ref_count } => {
                *ref_count -= 1;
                if *ref_count == 0 {
                    guard.status = FileDownloadStatus::Cached;
                }
            }
        }
    }

    fn fd(&self, flags: OFlags) -> Result<OwnedFd, io::Error> {
        Ok(fs::openat2(
            &self.store.root_fd,
            &self.key,
            flags,
            Mode::from_bits(0o666).unwrap(),
            ResolveFlags::BENEATH,
        )?)
    }
}

impl FileHandle {
    /// Wait until the length of the data downloaded for this file exceeds `offset`.
    /// Returns the actual length of data now available, which the hook will use
    /// as an upper bound for read syscalls.
    ///
    /// E.g. suppose a file is 1000 bytes long, the first 200 bytes are currently downloaded,
    /// and a program tries to read 100 bytes from offset 300. We have to block because otherwise
    /// the read would return some zeroes that we have allocated in the file. If the next chunk of
    /// data we download gets us to byte 350, then we can permit the read to go through but clamp
    /// the length to 50 so that it doesn't reach the file's zeroes.
    pub async fn read_past(&mut self, offset: u64) -> Result<u64, io::Error> {
        let Some(file) = self.file.upgrade() else {
            // file was deleted. hopefully it was downloaded fully. pretend the data's all there
            return Ok(u64::MAX);
        };
        let guard = file.state.lock().unwrap();
        match guard.status {
            FileDownloadStatus::Sparse | FileDownloadStatus::Cached => panic!(
                "handle existed for file {} in state {:?}",
                &file.key, *guard
            ),
            FileDownloadStatus::Downloading {
                ref tx,
                ref_count: _,
            } => {
                let mut rx = tx.subscribe();
                drop(guard);
                drop(file);
                loop {
                    match rx.recv().await {
                        Ok(available_offset) => {
                            if available_offset > offset {
                                return Ok(available_offset);
                            }
                        }
                        Err(RecvError::Closed) => {
                            // maybe file was deleted
                            // maybe download finished
                            return Ok(u64::MAX);
                        }
                        // this happens if so many messages were sent through the channel since we
                        // last called recv() that some of the oldest messages in the buffer had
                        // to be overwritten, so this receiver will skip over some messages.
                        //
                        // in our case it doesn't matter at all, we just wait for the next message.
                        Err(RecvError::Lagged(_)) => {}
                    }
                }
            }
            FileDownloadStatus::Complete { .. } => Ok(guard.length),
        }
    }
}

impl Drop for FileHandle {
    /// Give up this reference to the file
    fn drop(&mut self) {
        if let Some(file) = self.file.upgrade() {
            file.unref();
        }
    }
}
