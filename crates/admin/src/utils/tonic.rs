// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::pin::Pin;
use tonic::{Code, Response, Status};
use tonic_types::{ErrorDetails, StatusExt};
use tracing::{debug, error};

use crate::admin::registry::NotRegistered;

pub(crate) type Stream<T> =
    Pin<Box<dyn tokio_stream::Stream<Item = Result<T, Status>> + Send + 'static>>;

/// Is this error just "the caller asked about something not registered yet"?
///
/// Must go through `anyhow::Error::downcast_ref`, which knows how to look
/// inside the layers `.context()` adds. Walking `chain()` and testing each
/// `&dyn Error` with `is::<NotRegistered>()` does NOT work: the chain yields
/// anyhow's own wrapper around the context value, so the test is always false
/// and the classification silently does nothing.
fn is_expected_miss(err: &anyhow::Error) -> bool {
    err.downcast_ref::<NotRegistered>().is_some()
}

// Kludge: wrap_error have .into() semantic, so should be destructive
// Clippy hint here use &anyhow::Error, but implementing it trigger another clippy warning,
// suggests to pass by-value here.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn wrap_error(any_err: anyhow::Error) -> tonic::Status {
    // Convert root cause and stack to strings
    let stack: Vec<_> = any_err.chain().skip(1).map(ToString::to_string).collect();
    let cause = any_err.root_cause().to_string();

    // ...then dump them, at a level that matches how alarming they are.
    if is_expected_miss(&any_err) {
        debug!("Local error cause is {cause}");
        for each in &stack {
            debug!("Local reasons is {each}");
        }
    } else {
        error!("Local error cause is {cause}");
        for each in &stack {
            error!("Local reasons is {each}");
        }
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
    result.map(Response::new).wrap_error()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin::registry::Registry;

    /// The error a client actually provokes by asking about a VM that has not
    /// registered yet. Built through `Registry`, not by hand, so the test fails
    /// if `by_name` ever stops attaching the marker.
    #[test]
    fn miss_from_registry_is_expected() {
        let err = Registry::new()
            .by_name("givc-gui-vm.service")
            .expect_err("empty registry must not resolve a name");
        assert!(
            is_expected_miss(&err),
            "a registry miss must be classified as expected, got: {err:?}"
        );
    }

    #[test]
    fn unrelated_error_is_not_expected() {
        let err = anyhow::anyhow!("disk on fire").context("while doing something important");
        assert!(
            !is_expected_miss(&err),
            "an unrelated error must keep error-level logging"
        );
    }
}
