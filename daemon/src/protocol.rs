//! Message format:
//! - Header (4 bytes)
//!     - Payload size (u16)
//!     - Tag (u16)
//! - Payload (n bytes)
//!
//! All numbers are little-endian unless otherwise specified.
//!
//! Client messages:
//! - Tag 0: request to open a file. Payload is the path relative to the backing store.
//! - Tag 1: request to close a file. Payload is the path relative to the backing store.
//!
//! Server messages:
//! - Tag 0: response to open request. Payload:
//!     - 1 byte: 0 for success, 1 for error
//!     - 4 bytes: errno value (i32) (unspecified value if the request was successful)
//!     - Remainder: the path

use std::{
    io::{self, ErrorKind},
    string::FromUtf8Error,
};

use snafu::{ResultExt, prelude::Snafu};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tracing::trace;

#[derive(Debug)]
pub enum ClientMessage {
    Open { path: String },
    Close { path: String },
}

#[derive(Debug)]
pub enum ServerMessage<'a> {
    Opened { path: &'a str, error: Option<i32> },
    // TODO: does daemon need to acknowledge closes?
}

#[derive(Debug, Snafu)]
pub enum ReadMessageError {
    #[snafu(context(suffix(ReadSnafu)))]
    Io {
        source: io::Error,
    },
    UnknownTag {
        tag: u16,
    },
    InvalidString {
        source: FromUtf8Error,
    },
}

pub async fn read_message<S: AsyncRead + Unpin>(
    stream: &mut S,
) -> Result<Option<ClientMessage>, ReadMessageError> {
    let result = async {
        let payload_len = stream.read_u16_le().await.context(IoReadSnafu)?;
        let tag = stream.read_u16_le().await.context(IoReadSnafu)?;
        match tag {
            0 | 1 => {
                let mut buf = vec![0u8; payload_len as usize];
                stream.read_exact(&mut buf).await.context(IoReadSnafu)?;
                let path = String::try_from(buf).context(InvalidStringSnafu)?;
                Ok(match tag {
                    0 => ClientMessage::Open { path },
                    1 => ClientMessage::Close { path },
                    _ => unreachable!(),
                })
            }
            x => Err(ReadMessageError::UnknownTag { tag: x }),
        }
    }
    .await;
    match result {
        Ok(msg) => {
            trace!(?msg, "got message");
            Ok(Some(msg))
        }
        Err(err) => {
            if let ReadMessageError::Io { ref source } = err
                && source.kind() == ErrorKind::UnexpectedEof
            {
                Ok(None)
            } else {
                Err(err)
            }
        }
    }
}

#[derive(Debug, Snafu)]
pub enum WriteMessageError {
    TooBig,
    #[snafu(context(suffix(WriteSnafu)))]
    Io {
        source: io::Error,
    },
}

pub async fn write_message<'a, S: AsyncWrite + Unpin>(
    stream: &mut S,
    msg: &ServerMessage<'a>,
) -> Result<(), WriteMessageError> {
    trace!(?msg, "sending message");
    match msg {
        ServerMessage::Opened { path, error } => {
            let payload_size =
                u16::try_from(1 + 4 + path.len()).map_err(|_| WriteMessageError::TooBig)?;
            stream
                .write_u16_le(payload_size)
                .await
                .context(IoWriteSnafu)?;
            stream.write_u16_le(0).await.context(IoWriteSnafu)?;
            stream
                .write_u8(if error.is_none() { 0 } else { 1 })
                .await
                .context(IoWriteSnafu)?;
            stream
                .write_i32_le(error.unwrap_or(0))
                .await
                .context(IoWriteSnafu)?;
            stream
                .write_all(path.as_bytes())
                .await
                .context(IoWriteSnafu)?;
            Ok(())
        }
    }
}
