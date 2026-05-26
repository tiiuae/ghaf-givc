// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    Text,
    Jsonl,
}

#[derive(Debug, Parser)]
pub struct RegistryCommand {
    #[command(subcommand)]
    pub action: RegistryAction,

    /// Output format for progress and results
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub output: OutputFormat,

    /// Registry username (basic auth)
    #[arg(long)]
    pub username: Option<String>,

    /// Registry password (basic auth)
    #[arg(long, requires = "username")]
    pub password: Option<String>,

    /// Registry API token (bearer auth)
    #[arg(long, conflicts_with_all = ["username", "password"])]
    pub token: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum RegistryAction {
    /// Discover available OTA updates in OCI repository
    Discover {
        /// OCI reference to repository (registry/repo[/namespace])
        reference: String,
    },

    /// Pull OTA update artifacts from OCI repository
    Pull {
        /// OCI reference with tag (registry/repo[:tag])
        reference: String,

        /// Destination root path
        #[arg(long, default_value = "/persist/sysupdate")]
        destination: String,

        /// Validate pulled artifacts
        #[arg(long, conflicts_with = "no_validate")]
        validate: bool,

        /// Skip pulled artifacts validation
        #[arg(long, conflicts_with = "validate")]
        no_validate: bool,

        /// Apply installation immediately after successful pull
        #[arg(long)]
        install: bool,
    },

    /// Fetch changelog text for a specific tag
    Changelog {
        /// OCI reference with tag (registry/repo[:tag])
        reference: String,
    },
}

impl RegistryCommand {
    #[allow(clippy::missing_errors_doc)]
    pub async fn handle(self) -> anyhow::Result<()> {
        let _ = self;
        anyhow::bail!("registry CLI is not implemented yet")
    }
}
