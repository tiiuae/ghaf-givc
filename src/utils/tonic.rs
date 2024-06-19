use std::future::Future;
use std::result::Result;
use anyhow;
use tonic_types::{ErrorDetails, StatusExt};
use tonic::{Code, Request, Response, Status};


pub async fn escalate<T, R, F, FA>(
    req: tonic::Request<T>,
    fun: F,
) -> Result<tonic::Response<R>, tonic::Status>
where
    F: FnOnce(T) -> FA,
    FA: Future<Output = anyhow::Result<R>>,
{
    let result = fun(req.into_inner()).await;
    match result {
        std::result::Result::Ok(res) => std::result::Result::Ok(Response::new(res)),
        Err(any) => {
            let mut err_details = ErrorDetails::new();
            // Generate error status
            let status = Status::with_error_details(
                Code::InvalidArgument,
                "request contains invalid arguments",
                err_details,
            );

            return Err(status);
        }
    }
}
