# dls3

This is a program which makes an S3 (or other compatible object storage) bucket appear in the filesystem. Normally, you would do this with FUSE, but we want this to work in container environments where you can't set up FUSE. Instead, we insert an LD_PRELOAD hook into the target program which replaces C library functions for working with files.

There are two components:

- `hook` - the library that is linked into programs that should see the S3 bucket, written in Zig
- `daemon` - the program that runs in the background communicating with S3. `hook` sends requests to `daemon` over a UNIX socket and the functions return when the data is available.

## Terminology

- _victim_ is the program which has the library injected and will see the S3 bucket
- _mountpoint_ is the path where files appear to the victim
- _backing store_ is where real files are created on disk. Each instance of the daemon manages one backing store for one bucket.

## Configuration

The daemon is passed command-line arguments to specify the bucket, prefix, backing store, and credentials. The hook is passed environment variables specifying the mountpoint and how it should connect to the daemon.

## Operation

- When the daemon starts, it lists all objects in the bucket and populates the backing store with sparse files. These appear with the correct size but have no space allocated.
- The daemon also creates the mountpoint (if it doesn't already exist) as a symlink to `/proc/self/fd/$FD`, where `$FD` is a chosen file descriptor that we believe the hook will be able to obtain. The hook will use `fcntl` to obtain that file descriptor pointing to the backing store. In this way, file operations performed undre the mountpoint will actually work against the backing store, and all the hook needs to do is check whether the resolved path underlying a new file descriptor is under the backing store.
  - Many instances of the hook can use the same mountpoint to refer to different backing stores, as long as they are all able to obtain the same file descriptor. The file descriptor used should be somewhat large so that it is less likely to interfere with code passing file descriptors to child processes. My current uninformed game plan is to use half of `RLIMIT_NOFILE` (a limit which is one greater than the highest file descriptor a process can open).
- When a file is first opened, the daemon uses `fallocate` to reserve physical space for the file. If this fails, it tries to free the space used by other files in the backing store that are not actively used. Once space is reserved, it starts downloading the file from S3 and writing data to the backing file. The hook opens the backing file and returns the stream or file descriptor to the victim.
  - Right now, opening will block until the file is completely downloaded. In a later implementation, opening will succeed once space for the file is reserved and the download will proceed in the background, and then only reads and writes will block until the requested part of the file has been downloaded.
- When a file is written, the write is done against the backing file and the daemon marks the file as dirty. The modified version is uploaded to S3 shortly after it is closed.
- Periodically, the daemon checks S3 and downloads new versions of objects that have been modified.

## Limitations

- Objects larger than the disk used for the backing store do not work
- Writes are not synced to S3 until the file is closed
- Paths must be UTF-8
- The victim observes the mountpoint to be a symbolic link that resolves to the backing store. This creates less-than-intuitive behavior for paths that escape the mountpoint, e.g. if the mountpoint is `/mnt/neer` and the backing store is at `/home/neer/.local/state/store`, `/mnt/neer/../foo.txt` will point to `/home/neer/.local/state/foo.txt` instead of `/mnt/foo.txt`. But, paths containing `..` that don't escape the mountpoint (e.g. `/mnt/neer/foo/bar/../../baz`) work fine.

## Major tasks

- [x] Demo
- [x] Daemon knows when files are closed and can reclaim their space
- [ ] Preiodically check S3 for new/changed files
- [ ] Hook into more functions
- [ ] Handle writing files
