// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use clap::Parser;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = givc_agent::cli::Cli::parse();
    givc_agent::trace_init(cli.debug)?;
    let config = givc_agent::config::AgentConfig::load(&cli.config)?;
    tracing::info!(config = %cli.config.display(), "givc-agent cli parsed");
    givc_agent::runtime::AgentRuntime::from_config(config)?
        .serve()
        .await
}
