// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use anyhow::Context;

pub mod cli;
pub mod config;
pub mod ctap;
pub mod hwid;
pub mod locale;
pub mod runtime;
pub mod service;
pub mod servicemanager;
pub mod statsmanager;

/// Init logging.
///
/// # Errors
///
/// Will return `Err` if failed to initialize logging.
pub fn trace_init(debug: bool) -> anyhow::Result<()> {
    use std::env;
    use tracing::Level;
    use tracing_subscriber::{EnvFilter, Layer, layer::SubscriberExt};

    let env_filter = if debug && env::var("GIVC_LOG").is_err() {
        EnvFilter::from("debug")
    } else {
        EnvFilter::try_from_env("GIVC_LOG").unwrap_or_else(|_| EnvFilter::from("info"))
    };
    let is_debug_log_level = env_filter
        .max_level_hint()
        .map_or_else(|| false, |level| level >= Level::DEBUG);

    let output = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_target(is_debug_log_level)
        .with_file(is_debug_log_level)
        .with_line_number(is_debug_log_level)
        .with_thread_ids(is_debug_log_level);

    let output = if is_debug_log_level {
        output.pretty().boxed()
    } else {
        output.boxed()
    };

    if env::var("INVOCATION_ID").is_ok() {
        let journald = tracing_journald::layer()
            .map(|layer| layer.with_filter(env_filter.clone()).boxed())
            .unwrap_or(output.with_filter(env_filter).boxed());

        tracing::subscriber::set_global_default(tracing_subscriber::registry().with(journald))
            .context("tracing shouldn't already have been set up")?;
    } else {
        tracing::subscriber::set_global_default(
            tracing_subscriber::registry().with(output.with_filter(env_filter)),
        )
        .context("tracing shouldn't already have been set up")?;
    }

    Ok(())
}
