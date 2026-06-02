// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use clap::{Parser, Subcommand, ValueEnum};

use super::progress::{FeedbackSink, RegistryEvent};
use super::{
    DiscoverOptions, PullOptions, RegistryCredentials, discover_updates, fetch_changelog,
    prune_downloaded_updates, pull_update, push_update,
};

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

    /// Prune stale downloaded updates from destination root
    Prune {
        /// Destination root path
        #[arg(long, default_value = "/persist/sysupdate")]
        destination: String,
    },

    /// Push OTA update artifacts to OCI repository
    Push {
        /// Path to manifest file
        #[arg(long)]
        manifest: String,

        /// OCI reference with tag (registry/repo[:tag])
        reference: String,

        /// Optional changelog file path
        #[arg(long)]
        changelog: Option<String>,
    },
}

impl RegistryCommand {
    #[allow(clippy::missing_errors_doc)]
    pub async fn handle(self) -> anyhow::Result<()> {
        let credentials = self.credentials()?;
        let mut sink = CliFeedback {
            format: self.output,
        };

        match self.action {
            RegistryAction::Discover { reference } => {
                let updates = discover_updates(
                    &DiscoverOptions {
                        reference,
                        credentials,
                    },
                    &mut sink,
                    None,
                )
                .await?;

                match self.output {
                    OutputFormat::Text => {
                        for update in updates {
                            println!(
                                "{}/{} version={} hash={}",
                                update.repository,
                                update.tag,
                                update.version,
                                short_hash(&update.hash)
                            );
                        }
                    }
                    OutputFormat::Jsonl => {
                        for update in updates {
                            println!("{}", serde_json::to_string(&update)?);
                        }
                    }
                }
                Ok(())
            }
            RegistryAction::Pull {
                reference,
                destination,
                validate,
                no_validate,
                install,
            } => {
                let result = pull_update(
                    &PullOptions {
                        reference,
                        destination_root: destination.into(),
                        credentials,
                        install,
                        validate: validate && !no_validate,
                    },
                    &mut sink,
                    None,
                )
                .await?;

                match self.output {
                    OutputFormat::Text => {
                        println!("pulled to: {}", result.output_dir.display());
                        println!("manifest: {}", result.manifest_path.display());
                    }
                    OutputFormat::Jsonl => {
                        println!("{}", serde_json::to_string(&result.output_dir)?);
                        println!("{}", serde_json::to_string(&result.manifest_path)?);
                    }
                }
                Ok(())
            }
            RegistryAction::Changelog { reference } => {
                let changelog = fetch_changelog(&reference, &credentials, None).await?;
                println!("{changelog}");
                Ok(())
            }
            RegistryAction::Prune { destination } => {
                prune_downloaded_updates(&super::PruneOptions {
                    destination_root: destination.into(),
                })
                .await?;
                if matches!(self.output, OutputFormat::Text) {
                    println!("prune done");
                }
                Ok(())
            }
            RegistryAction::Push {
                manifest,
                reference,
                changelog,
            } => {
                let result = push_update(&super::PushOptions {
                    reference,
                    manifest_path: manifest.into(),
                    changelog_path: changelog.map(Into::into),
                    credentials,
                })
                .await?;
                println!(
                    "pushed: {} manifest_url={} digest={}",
                    result.reference, result.manifest_url, result.digest
                );
                Ok(())
            }
        }
    }

    fn credentials(&self) -> anyhow::Result<RegistryCredentials> {
        if let Some(token) = &self.token {
            return Ok(RegistryCredentials::Bearer {
                token: token.clone(),
            });
        }

        match (&self.username, &self.password) {
            (Some(username), Some(password)) => Ok(RegistryCredentials::Basic {
                username: username.clone(),
                password: password.clone(),
            }),
            (None, None) => Ok(RegistryCredentials::Anonymous),
            (Some(_), None) => anyhow::bail!("--password is required when --username is set"),
            (None, Some(_)) => anyhow::bail!("--username is required when --password is set"),
        }
    }
}

struct CliFeedback {
    format: OutputFormat,
}

impl FeedbackSink for CliFeedback {
    fn event(&mut self, event: RegistryEvent) {
        match self.format {
            OutputFormat::Text => print_text_event(&event),
            OutputFormat::Jsonl => {
                if let Ok(line) = serde_json::to_string(&event) {
                    println!("{line}");
                }
            }
        }
    }
}

fn print_text_event(event: &RegistryEvent) {
    match event {
        RegistryEvent::DiscoverStarted { reference, total } => {
            println!("discover start: {reference} ({total} tags)");
        }
        RegistryEvent::TagDiscovered {
            repository,
            tag,
            current,
            total,
        } => {
            println!("tag [{current}/{total}]: {repository}:{tag}");
        }
        RegistryEvent::ManifestFetched {
            repository,
            tag,
            current,
            total,
        } => {
            println!("manifest [{current}/{total}]: {repository}:{tag}");
        }
        RegistryEvent::Done => {
            println!("done");
        }
        RegistryEvent::PullStarted {
            reference,
            destination,
        } => {
            println!("pull start: {reference} -> {destination}");
        }
        _ => {}
    }
}

fn short_hash(value: &str) -> &str {
    value.get(..16).unwrap_or(value)
}
