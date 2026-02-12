// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use reqwest::StatusCode;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CachixError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Unauthorized (401)")]
    Unauthorized,

    #[error("Unexpected status code: {0}")]
    UnexpectedStatus(StatusCode),
}
