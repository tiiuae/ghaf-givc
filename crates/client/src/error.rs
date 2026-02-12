// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use anyhow::Error;
use tonic::Status;
use tonic_types::StatusExt;

pub(crate) fn rewrap_error(status: &Status) -> Error {
    let mut err = Error::msg(status.message().to_owned());
    let details = status.get_error_details();
    if let Some(debug_info) = details.debug_info() {
        err = err.context(format!("Detail: {}", debug_info.detail));
        err = debug_info
            .stack_entries
            .iter()
            .fold(err, |err, each| err.context(format!("Stack: {each}")));
    }
    err
}

pub trait StatusWrapExt<T> {
    /// # Errors
    /// Return `Err(anyhow::Error)` if `tonic::Status` contains error
    fn rewrap_err(self) -> Result<T, Error>;
}

impl<T> StatusWrapExt<T> for Result<T, Status> {
    fn rewrap_err(self) -> Result<T, Error> {
        self.map_err(|status| rewrap_error(&status))
    }
}
