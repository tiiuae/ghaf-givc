use anyhow;
use std::future::Future;
use tonic::{Code, Response, Status};
use tonic_types::{ErrorDetails, StatusExt};
use tracing::error;

/// Wrap function `fun` converting unwrapping incoming `tonic::Request<T>`
/// Also rewrap result, processing error conversion from `anyhow` to `tonic`
/// # Errors
/// Return `Err(tonic::Status)` if inner function fails
pub async fn escalate<T, R, F, FA>(
    request: tonic::Request<T>,
    fun: F,
) -> Result<tonic::Response<R>, tonic::Status>
where
    F: FnOnce(T) -> FA,
    FA: Future<Output = anyhow::Result<R>>,
{
    let result = fun(request.into_inner()).await;
    match result {
        Ok(res) => Ok(Response::new(res)),
        Err(any_err) => {
            // Convert root cause and stack to strings
            let stack: Vec<_> = any_err.chain().skip(1).map(ToString::to_string).collect();
            let cause = any_err.root_cause().to_string();

            // ...then dump them...
            error!("Local error cause is {cause}");
            for e in &stack {
                error!("Local reasons is {e}");
            }

            // ...then pack to ErrorDetails
            let err_details = ErrorDetails::with_debug_info(stack, cause);
            // Generate error status
            let status = Status::with_error_details(
                Code::InvalidArgument,
                "request contains invalid arguments",
                err_details,
            );
            error!("error handling GRPC request: {any_err}");

            Err(status)
        }
    }
}
