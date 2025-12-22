use anyhow::Context;

pub mod admin;
pub mod policyagent_api;
pub mod systemd_api;
pub mod utils;

pub mod pb {
    // Re-export to keep current code untouched
    pub use givc_common::pb::*;
}
pub use givc_client::endpoint;
pub use givc_common::types;

/// Init logging
///
/// # Errors
///
/// Will return `Err` if failed to initialize logging
pub fn trace_init() -> anyhow::Result<()> {
    use std::env;
    use tracing::Level;
    use tracing_subscriber::{EnvFilter, Layer, filter::LevelFilter, layer::SubscriberExt};

    let env_filter =
        EnvFilter::try_from_env("GIVC_LOG").unwrap_or_else(|_| EnvFilter::from("info"));
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

    // enable journald logging only on release to avoid log spam on dev machines
    let journald = match env::var("INVOCATION_ID") {
        Err(_) => None,
        Ok(_) => tracing_journald::layer().ok(),
    };

    let subscriber = tracing_subscriber::registry()
        .with(journald.with_filter(LevelFilter::INFO))
        .with(output.with_filter(env_filter));

    tracing::subscriber::set_global_default(subscriber)
        .context("tracing shouldn't already have been set up")?;
    Ok(())
}
