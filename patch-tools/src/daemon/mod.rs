pub mod client;
mod engine_worker;
mod resolver;
pub mod runtime;
pub mod server;

use crate::types::{DaemonRequest, DaemonResponse};
use anyhow::{Context, Result};
use prost::Message;
use std::io::ErrorKind;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub async fn read_request<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Option<DaemonRequest>> {
    let Some(bytes) = read_frame(reader).await? else {
        return Ok(None);
    };
    let request = DaemonRequest::decode(bytes.as_slice()).context("invalid request protobuf")?;
    request
        .kind_ref()
        .context("invalid request protobuf: missing request kind")?;
    Ok(Some(request))
}

pub async fn write_request<W: AsyncWrite + Unpin>(
    writer: &mut W,
    request: &DaemonRequest,
) -> Result<()> {
    request.kind_ref()?;
    write_message(writer, request).await
}

pub async fn read_response<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Option<DaemonResponse>> {
    let Some(bytes) = read_frame(reader).await? else {
        return Ok(None);
    };
    let response = DaemonResponse::decode(bytes.as_slice()).context("invalid response protobuf")?;
    response
        .kind_ref()
        .context("invalid response protobuf: missing response kind")?;
    Ok(Some(response))
}

pub async fn write_response<W: AsyncWrite + Unpin>(
    writer: &mut W,
    response: &DaemonResponse,
) -> Result<()> {
    response.kind_ref()?;
    write_message(writer, response).await
}

async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Option<Vec<u8>>> {
    let frame_len = match reader.read_u32().await {
        Ok(frame_len) => frame_len as usize,
        Err(error) if error.kind() == ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error).context("failed to read protobuf frame length"),
    };

    let mut bytes = vec![0; frame_len];
    reader
        .read_exact(&mut bytes)
        .await
        .context("failed to read protobuf frame payload")?;
    Ok(Some(bytes))
}

async fn write_message<W: AsyncWrite + Unpin, M: Message>(
    writer: &mut W,
    message: &M,
) -> Result<()> {
    let mut bytes = Vec::with_capacity(message.encoded_len());
    message
        .encode(&mut bytes)
        .context("failed to encode protobuf message")?;

    let frame_len =
        u32::try_from(bytes.len()).context("protobuf frame exceeds 4-byte length prefix")?;
    writer
        .write_u32(frame_len)
        .await
        .context("failed to write protobuf frame length")?;
    writer
        .write_all(&bytes)
        .await
        .context("failed to write protobuf frame payload")?;
    writer
        .flush()
        .await
        .context("failed to flush protobuf frame")?;

    Ok(())
}
