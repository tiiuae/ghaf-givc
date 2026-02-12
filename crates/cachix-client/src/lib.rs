// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

pub mod client;
pub mod config;
pub mod error;
pub mod types;

#[cfg(feature = "nixos")]
pub mod nixos;

pub use client::CachixClient;
pub use config::CachixClientConfig;
pub use error::CachixError;
pub use types::*;
