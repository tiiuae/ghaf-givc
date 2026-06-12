// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use clap::{Parser, Subcommand, ValueEnum};
use oci_client::client::ClientProtocol;
use std::path::PathBuf;

use super::progress::RegistryEvent;
use super::set_client_protocol;
use super::{
    DiscoverOptions, PullOptions, RegistryCredentials, TaggedReference, UntaggedReference,
    discover_updates, fetch_changelog, prune_downloaded_updates, pull_update,
    push_update_with_feedback,
};

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    Text,
    Jsonl,
}

#[derive(Debug, Parser)]
#[group(requires_all = ["username", "password"])]
pub struct PasswordAuth {
    /// Registry username (basic auth)
    #[arg(long, required = false)]
    pub username: String,

    /// Registry password (basic auth)
    #[arg(long, required = false)]
    pub password: String,
}

#[derive(Debug, Parser)]
pub struct RegistryCommand {
    #[command(subcommand)]
    pub action: RegistryAction,

    /// Output format for progress and results
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub output: OutputFormat,

    /// Registry username/password (basic auth)
    #[clap(flatten)]
    pub auth: Option<PasswordAuth>,

    /// Registry API token (bearer auth)
    #[arg(long, conflicts_with_all = ["username", "password"])]
    pub token: Option<String>,

    /// Use HTTP instead of HTTPS for registry access
    #[arg(long)]
    pub insecure: bool,
}

#[derive(Debug, Subcommand)]
pub enum RegistryAction {
    /// Discover available OTA updates in OCI repository
    Discover {
        /// OCI reference to repository (registry/repo[/namespace])
        reference: UntaggedReference,
    },

    /// Pull OTA update artifacts from OCI repository
    Pull {
        /// OCI reference with tag (registry/repo[:tag])
        reference: TaggedReference,

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
        reference: TaggedReference,
    },

    /// Prune stale downloaded updates from destination root
    Prune {
        /// Destination root path
        #[arg(long, default_value = "/persist/sysupdate")]
        destination: PathBuf,
    },

    /// Push OTA update artifacts to OCI repository
    Push {
        /// Path to manifest file
        #[arg(long)]
        manifest: PathBuf,

        /// OCI reference with tag (registry/repo[:tag])
        reference: TaggedReference,

        /// Optional changelog file path
        #[arg(long)]
        changelog: Option<PathBuf>,
    },
}

impl RegistryCommand {
    #[allow(clippy::missing_errors_doc)]
    pub async fn handle(self) -> anyhow::Result<()> {
        let credentials = self.credentials();
        if self.insecure {
            set_client_protocol(ClientProtocol::Http);
        }
        let (feedback_tx, feedback_rx) = async_channel::unbounded();
        let progress_task = tokio::spawn(feedback_printer(self.output, feedback_rx));

        let result = match self.action {
            RegistryAction::Discover { reference } => {
                let updates = discover_updates(
                    &DiscoverOptions {
                        reference,
                        credentials,
                    },
                    Some(&feedback_tx),
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
                        println!("{}", serde_json::to_string(&updates)?);
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
                    Some(&feedback_tx),
                    None,
                )
                .await?;

                match self.output {
                    OutputFormat::Text => {
                        println!("pulled to: {}", result.output_dir.display());
                        println!("manifest: {}", result.manifest_path.display());
                    }
                    OutputFormat::Jsonl => {
                        println!("{}", serde_json::to_string(&result)?);
                    }
                }
                Ok(())
            }
            RegistryAction::Changelog { reference } => {
                let changelog =
                    fetch_changelog(&reference, &credentials, Some(&feedback_tx), None).await?;
                println!("{changelog}");
                Ok(())
            }
            RegistryAction::Prune { destination } => {
                prune_downloaded_updates(&super::PruneOptions {
                    destination_root: destination,
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
                let result = push_update_with_feedback(
                    &super::PushOptions {
                        reference,
                        manifest_path: manifest,
                        changelog_path: changelog,
                        credentials,
                    },
                    Some(&feedback_tx),
                )
                .await?;
                println!(
                    "pushed: {} manifest_url={} digest={}",
                    result.reference, result.manifest_url, result.digest
                );
                Ok(())
            }
        };
        drop(feedback_tx);
        progress_task
            .await
            .map_err(|err| anyhow::anyhow!("progress printer task failed: {err}"))?;
        result
    }

    fn credentials(&self) -> RegistryCredentials {
        if let Some(token) = &self.token {
            return RegistryCredentials::Bearer {
                token: token.clone(),
            };
        }
        if let Some(auth) = &self.auth {
            return RegistryCredentials::Basic {
                username: auth.username.clone(),
                password: auth.password.clone(),
            };
        }

        RegistryCredentials::Anonymous
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
        RegistryEvent::PushStarted { reference, layers } => {
            println!("push start: {reference} ({layers} layers)");
        }
        RegistryEvent::LayerUploading {
            kind,
            uploaded,
            total,
        } => {
            println!("upload {kind}: {uploaded}/{}", total.unwrap_or(0));
        }
        RegistryEvent::LayerUploaded { kind, digest } => {
            println!("uploaded {kind}: {digest}");
        }
        RegistryEvent::ManifestPushed {
            reference,
            manifest_url,
            digest,
        } => {
            println!("push done: {reference} url={manifest_url} digest={digest}");
        }
        _ => {}
    }
}

async fn feedback_printer(format: OutputFormat, rx: async_channel::Receiver<RegistryEvent>) {
    while let Ok(event) = rx.recv().await {
        match format {
            OutputFormat::Text => print_text_event(&event),
            OutputFormat::Jsonl => {
                if let Ok(line) = serde_json::to_string(&event) {
                    println!("{line}");
                }
            }
        }
    }
}

fn short_hash(value: &str) -> &str {
    value.get(..16).unwrap_or(value)
}
