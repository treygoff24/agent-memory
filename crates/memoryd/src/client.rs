use std::path::Path;

use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use crate::protocol::{RequestEnvelope, RequestPayload, ResponseEnvelope, MAX_FRAME_BYTES};

pub async fn request(
    socket_path: impl AsRef<Path>,
    request_id: impl Into<String>,
    request: RequestPayload,
) -> Result<ResponseEnvelope> {
    let socket_path = socket_path.as_ref();
    let stream = UnixStream::connect(socket_path)
        .await
        .with_context(|| format!("connect to memoryd socket {}", socket_path.display()))?;
    let mut stream = BufReader::with_capacity(MAX_FRAME_BYTES, stream);
    let request = RequestEnvelope::new(request_id, request);
    stream
        .get_mut()
        .write_all(request.to_json_line().context("serialize daemon request")?.as_bytes())
        .await
        .context("write daemon request")?;

    let mut line = String::new();
    let bytes = stream.take(MAX_FRAME_BYTES as u64).read_line(&mut line).await.context("read daemon response")?;
    if bytes == 0 {
        anyhow::bail!("memoryd closed socket without a response");
    }
    ResponseEnvelope::from_json_line(&line).context("decode daemon response")
}
