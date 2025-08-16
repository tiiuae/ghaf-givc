use anyhow;
use std::pin::Pin;
use tonic::{Code, Response, Status};
use tonic_types::{ErrorDetails, StatusExt};
use tracing::error;

pub(crate) type Stream<T> =
    Pin<Box<dyn tokio_stream::Stream<Item = Result<T, Status>> + Send + 'static>>;

// Kludge: wrap_error have .into() semantic, so should be destructive
// Clippy hint here use &anyhow::Error, but implementing it trigger another clippy warning,
// suggests to pass by-value here.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn wrap_error(any_err: anyhow::Error) -> tonic::Status {
    // Convert root cause and stack to strings
    let stack: Vec<_> = any_err.chain().skip(1).map(ToString::to_string).collect();
    let cause = any_err.root_cause().to_string();

    // ...then dump them...
    error!("Local error cause is {cause}");
    for each in &stack {
        error!("Local reasons is {each}");
    }

    // ...then pack to ErrorDetails
    let err_details = ErrorDetails::with_debug_info(stack, cause);
    // Generate error status
    Status::with_error_details(
        Code::InvalidArgument,
        "request contains invalid arguments",
        err_details,
    )
}

pub(crate) trait WrapError<T> {
    #[allow(clippy::result_large_err)]
    fn wrap_error(self) -> Result<T, tonic::Status>;
}

impl<T> WrapError<T> for Result<T, anyhow::Error> {
    fn wrap_error(self) -> Result<T, tonic::Status> {
        self.map_err(wrap_error)
    }
}

/// Wrap function `fun` converting unwrapping incoming `tonic::Request<T>`
/// Also rewrap result, processing error conversion from `anyhow` to `tonic`
/// # Errors
/// Return `Err(tonic::Status)` if inner function fails
pub async fn escalate<T, R>(
    req: tonic::Request<T>,
    fun: impl AsyncFnOnce(T) -> anyhow::Result<R>,
) -> Result<tonic::Response<R>, tonic::Status> {
    let result = fun(req.into_inner()).await;
    match result {
        Ok(res) => Ok(Response::new(res)),
        Err(any_err) => {
            error!("error handling GRPC request: {any_err}");
            let status = wrap_error(any_err);
            Err(status)
        }
    }
}
