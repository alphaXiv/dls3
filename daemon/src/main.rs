use std::{io::ErrorKind, str::FromStr};

use snafu::{ResultExt, whatever};
use tokio::{io::AsyncWriteExt, net::UnixListener};
use tracing::{info, warn};
use tracing_subscriber::{EnvFilter, Layer, layer::SubscriberExt};

use crate::{
    config::{Config, usage},
    connection::handle_client,
    protocol::{ClientMessage, read_message},
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

    let base_dir = dirs::cache_dir()
        .expect("failed to get cache directory")
        .join("dls3");
    tokio::fs::create_dir_all(&base_dir)
        .await
        .with_whatever_context(|_| format!("failed to create {base_dir:?}"))?;
    // clean up from previous instance
    let backing_path = base_dir.join("store");
    let socket_path = base_dir.join("dls3.sock");
    if let Err(err) = tokio::fs::remove_dir_all(&backing_path).await
        && err.kind() != ErrorKind::NotFound
    {
        whatever!(Err(err), "failed to remove {backing_path:?}");
    }
    if let Err(err) = tokio::fs::remove_file(&socket_path).await
        && err.kind() != ErrorKind::NotFound
    {
        whatever!(Err(err), "failed to remove {socket_path:?}");
    }

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

    let env = format!(
        concat!(
            "# pid={pid}\n", // this line is read by neer to check if the daemon is alive
            "export LD_PRELOAD={hook_path}:$LD_PRELOAD\n",
            "export DLS3_SOCKET_PATH={socket_path}\n",
            "export DLS3_BACKING_PATH={backing_path}\n",
            "export DLS3_BACKING_FD={backing_fd}\n",
        ),
        pid = std::process::id(),
        hook_path = &config.hook_path,
        socket_path = socket_path.to_str().expect("socket_path is invalid UTF-8"),
        backing_path = backing_path
            .to_str()
            .expect("backing_path is invalid UTF-8"),
        backing_fd = backing_fd,
    );

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
        .whatever_context("initializing backing store failed")?;

    info!(
        "created backing store in {}",
        backing_path.to_string_lossy()
    );

    let listen_loop_store = store.clone();
    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    info!("accepted listener");
                    let local_store = listen_loop_store.clone();
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

    // listen for auth messages on stdin
    let mut handle = store.handle();
    while let Some(msg) = read_message(&mut tokio::io::stdin())
        .await
        .whatever_context("failed to read stdin")?
    {
        if let ClientMessage::UpdateAuth(new_auth) = msg {
            handle.replace_auth(new_auth);
        }
    }

    std::process::exit(0)
}
