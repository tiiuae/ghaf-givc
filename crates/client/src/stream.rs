use super::error::rewrap_error;
use anyhow::{Context, anyhow, bail};
use tokio_stream::StreamExt;
use tonic::{Code, Status, Streaming};

/// Check "trailer status" in stream (if exists) and escalate it as an `Err`
pub(crate) async fn check_trailers<T>(mut stream: Streaming<T>) -> anyhow::Result<()> {
    if let Some(trailers) = stream.trailers().await? {
        let headers = trailers.into_headers();
        if let Some(status) = Status::from_header_map(&headers) {
            if status.code() != Code::Ok {
                return Err(rewrap_error(&status));
            }
        } else {
            bail!("invalid grpc-status trailer");
        }
    }
    Ok(())
}

/// Process stream of messages `T` with error propagation and proper handling of streamin error
/// # Errors
/// * Fails if subsequent callback call return an error
/// * Fails if stream bring error message
/// * Fails if streaming RPC call failed after last message
pub async fn drain_stream_with_callback<T, F>(
    mut stream: Streaming<T>,
    mut callback: F,
) -> anyhow::Result<()>
where
    T: Send + 'static,
    F: AsyncFnMut(T) -> anyhow::Result<()>,
{
    while let Some(msg) = stream.next().await {
        match msg {
            Ok(item) => callback(item).await?,
            Err(status) => return Err(anyhow!(rewrap_error(&status))),
        }
    }

    // Cover rare case, when last message already sent, but RPC call finished with error
    check_trailers(stream)
        .await
        .context("While check trailers")?;

    Ok(())
}
