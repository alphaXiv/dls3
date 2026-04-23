//! Functions for dealing with a client connection

use std::sync::Arc;

use snafu::{ResultExt, Whatever};
use tokio::net::UnixStream;

use crate::{
    protocol::{ClientMessage, ServerMessage, read_message, write_message},
    store::Store,
};

pub async fn handle_client(mut stream: UnixStream, store: Arc<Store>) -> Result<(), Whatever> {
    let mut handle = store.handle();
    while let Some(msg) = read_message(&mut stream)
        .await
        .whatever_context("could not parse client message")?
    {
        match msg {
            ClientMessage::Open { path } => {
                let result = handle.open_file(&path).await;
                write_message(
                    &mut stream,
                    &ServerMessage::Opened {
                        path: &path,
                        error: result.err(),
                    },
                )
                .await
                .whatever_context("could not send message")?;
            }
            ClientMessage::Close { .. } => todo!(),
        }
    }
    Ok(())
}
