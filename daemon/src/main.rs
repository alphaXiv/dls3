use std::{io::ErrorKind, str::FromStr};

use snafu::{ResultExt, whatever};
use tokio::net::UnixListener;
use tracing::{info, warn};
use tracing_subscriber::{EnvFilter, Layer, layer::SubscriberExt};

use crate::{config::CONFIG, connection::handle_client, s3::create_client, store::Store};

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
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::from_str("warn,daemon=trace").unwrap()),
        ),
    );
    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default tracing subscriber failed");

    let client = create_client(&CONFIG);

    let store = Store::new(&client, CONFIG.clone())
        .await
        .with_whatever_context(|_| "initializing backing store failed")?;

    info!("created backing store in {}", CONFIG.backing_path);

    match tokio::fs::remove_file(CONFIG.socket_path.as_ref()).await {
        Ok(()) => {}
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => whatever!(
            Err(err),
            "failed to unlink socket at {}",
            CONFIG.socket_path
        ),
    };
    let listener = UnixListener::bind(CONFIG.socket_path.as_ref())
        .with_whatever_context(|_| format!("could not listen at {}", CONFIG.socket_path))?;

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
