use std::{
    ffi::OsString, io::ErrorKind, os::unix::process::ExitStatusExt, process::Stdio, str::FromStr,
};

use snafu::{ResultExt, whatever};
use tokio::{net::UnixListener, signal::unix::SignalKind};
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
    let backing_path = base_dir.join("store");
    let socket_path = base_dir.join("dls3.sock");

    let store = Store::new(&client, config.clone(), backing_path.clone())
        .await
        .with_whatever_context(|_| "initializing backing store failed")?;

    info!(
        "created backing store in {}",
        backing_path.to_string_lossy()
    );

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

    let mut child = tokio::process::Command::new(&config.command[0])
        .args(&config.command[1..])
        .env(
            "LD_PRELOAD",
            if let Some(existing_preload) = std::env::var_os("LD_PRELOAD") {
                let mut val = OsString::from_str(&config.hook_path).unwrap();
                val.push(":");
                val.push(existing_preload);
                val
            } else {
                OsString::from_str(&config.hook_path).unwrap()
            },
        )
        .env("DLS3_SOCKET_PATH", &socket_path)
        .env("DLS3_BACKING_PATH", &backing_path)
        .env("DLS3_BACKING_FD", backing_fd.to_string())
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .with_whatever_context(|_| format!("failed to spawn {:?}", config.command))?;

    tokio::spawn(async move {
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
    });

    // ignore SIGINT in case the child wants to block it (if it doesn't, it will still exit)
    match tokio::signal::unix::signal(SignalKind::interrupt()) {
        Ok(_) => {}
        Err(err) => warn!(?err, "failed to listen for SIGINT"),
    }

    let status = child
        .wait()
        .await
        .whatever_context("failed to wait for child to exit")?;

    if let Err(err) = tokio::fs::remove_dir_all(&base_dir).await {
        warn!(?err, "failed to clean up files");
    }

    std::process::exit(
        status
            .code()
            .unwrap_or_else(|| status.signal().unwrap_or(0) + 128),
    );
}
