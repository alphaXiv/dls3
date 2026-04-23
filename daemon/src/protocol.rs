use std::{
    io::{self, ErrorKind},
    string::FromUtf8Error,
};

use snafu::{ResultExt, prelude::Snafu};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tracing::trace;

#[derive(Debug)]
pub enum ClientMessage {
    Open {
        path: String,
    },
    #[expect(dead_code)]
    Close {
        path: String,
    },
}

#[derive(Debug)]
pub enum ServerMessage {
    Opened { errno: u16 },
}

#[derive(Debug, Snafu)]
pub enum ReadMessageError {
    #[snafu(context(suffix(ReadSnafu)))]
    Io {
        source: io::Error,
    },
    UnknownTag {
        tag: u8,
    },
    InvalidString {
        source: FromUtf8Error,
    },
}

pub async fn read_message<S: AsyncRead + Unpin>(
    stream: &mut S,
) -> Result<Option<ClientMessage>, ReadMessageError> {
    let result = async {
        let tag = stream.read_u8().await.context(IoReadSnafu)?;
        let payload_len = stream.read_u16_le().await.context(IoReadSnafu)?;
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
        Err(ReadMessageError::Io { ref source }) if source.kind() == ErrorKind::UnexpectedEof => {
            Ok(None)
        }
        Err(err) => Err(err),
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

pub async fn write_message<S: AsyncWrite + Unpin>(
    stream: &mut S,
    msg: &ServerMessage,
) -> Result<(), WriteMessageError> {
    trace!(?msg, "sending message");
    match msg {
        ServerMessage::Opened { errno } => {
            // tag 0, len 2 (LE)
            stream.write_all(&[0, 2, 0]).await.context(IoWriteSnafu)?;
            stream.write_u16_le(*errno).await.context(IoWriteSnafu)?;
            Ok(())
        }
    }
}
