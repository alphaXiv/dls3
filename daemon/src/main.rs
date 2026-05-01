use std::{ffi::OsString, io::ErrorKind, os::unix::ffi::OsStrExt, str::FromStr};

use snafu::{ResultExt, whatever};
use tokio::{io::AsyncWriteExt, net::UnixListener};
use tracing::{info, warn};
use tracing_subscriber::{EnvFilter, Layer, layer::SubscriberExt};

use crate::{
    config::{Config, usage},
    connection::handle_client,
    s3::create_client,
    store::Store,
};

mod config;
mod connection;
mod protocol;
mod s3;
mod store;

#[tokio::main]
#[snafu::report]
async fn main() -> Result<(), snafu::Whatever> {
    let subscriber = tracing_subscriber::Registry::default().with(
        tracing_subscriber::fmt::Layer::default().with_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                EnvFilter::from_str(if cfg!(debug_assertions) {
                    "warn,daemon=trace"
                } else {
                    "warn"
                })
                .unwrap()
            }),
        ),
    );
    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default tracing subscriber failed");

    let config = Config::parse_args().with_whatever_context(|_| {
        usage();
        "invalid arguments"
    })?;

    let client = create_client(&config);

    let mut base_dir = dirs::cache_dir().expect("failed to get cache directory");
    base_dir.push(format!("dls3-{}", std::process::id()));
    tokio::fs::create_dir_all(&base_dir)
        .await
        .with_whatever_context(|_| format!("failed to create {base_dir:?}"))?;
    let backing_path = base_dir.join("store");
    let socket_path = base_dir.join("dls3.sock");

    // make the mountpoint a symlink to `/proc/self/fd/{some fd}`, or determine the file descriptor
    // if the mountpoint already exists
    let max_fd = rustix::process::getrlimit(rustix::process::Resource::Nofile)
        .current
        .expect("max file descriptor was unlimited");
    // we need to choose a file descriptor that every victim process will be able to open.
    // half of the maximum is unlikely to be taken up by something else.
    let mut backing_fd = max_fd / 2;

    match tokio::fs::symlink(format!("/proc/self/fd/{backing_fd}"), &config.mountpoint).await {
        Ok(()) => {}
        Err(err) if err.kind() == ErrorKind::AlreadyExists => {
            // maybe already set up by another daemon. check if it points to a suitable location.
            let destination = tokio::fs::read_link(&config.mountpoint)
                .await
                .whatever_context("failed to read mountpoint link")?;
            if let Ok(rest) = destination.strip_prefix("/proc/self/fd/")
                && let Some(rest_str) = rest.to_str()
                && let Ok(found_fd) = u64::from_str(rest_str)
            {
                backing_fd = found_fd;
            } else {
                whatever!(
                    "mountpoint is a symlink to {}, not /proc/self/fd/ + a file descriptor",
                    destination.to_string_lossy()
                );
            }
        }
        Err(err) => whatever!(
            Err(err),
            "failed to create symlink at {}",
            config.mountpoint
        ),
    };

    let listener = UnixListener::bind(&socket_path).with_whatever_context(|_| {
        format!("could not listen at {}", socket_path.to_string_lossy())
    })?;

    let mut env = OsString::new();
    env.push("LD_PRELOAD=");
    env.push(&config.hook_path);
    env.push(":$LD_PRELOAD\nDLS3_SOCKET_PATH=");
    env.push(&socket_path);
    env.push("\nDLS3_BACKING_PATH=");
    env.push(&backing_path);
    env.push("\nDLS3_BACKING_FD=");
    env.push(backing_fd.to_string());
    env.push("\n");

    let mut env_file = tokio::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&config.env_destination)
        .await
        .with_whatever_context(|_| format!("could not open {}", config.env_destination))?;
    env_file
        .write_all(env.as_bytes())
        .await
        .with_whatever_context(|_| format!("could not write to {}", config.env_destination))?;

    let store = Store::new(&client, config.clone(), backing_path.clone())
        .await
        .with_whatever_context(|_| "initializing backing store failed")?;

    info!(
        "created backing store in {}",
        backing_path.to_string_lossy()
    );

    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                info!("accepted listener");
                let local_store = store.clone();
                tokio::spawn(async move {
                    if let Err(err) = handle_client(stream, local_store).await {
                        warn!(?err, "error handling client connection");
                    }
                });
            }
            Err(err) => {
                warn!(?err, "error accepting connection");
            }
        }
    }
}
