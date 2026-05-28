// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::Parser;
use ota_update::image::install::validate_manifest_path;
use ota_update::registry::progress::NoopFeedback;
use ota_update::registry::{
    PullOptions, PushOptions, RegistryCredentials, pull_update, push_update,
};

#[derive(Debug, Parser)]
struct Args {
    #[arg(long)]
    manifest: PathBuf,

    #[arg(long)]
    reference: String,

    #[arg(long)]
    changelog: Option<PathBuf>,

    #[arg(long, default_value_t = false)]
    allow_nonlocal: bool,

    #[arg(long, default_value_t = false)]
    cleanup: bool,

    #[arg(long)]
    username: Option<String>,

    #[arg(long)]
    password: Option<String>,

    #[arg(long)]
    token: Option<String>,
}

fn validate_reference_host(reference: &str, allow_nonlocal: bool) -> anyhow::Result<()> {
    let host = reference
        .split('/')
        .next()
        .ok_or_else(|| anyhow::anyhow!("invalid reference"))?;
    if allow_nonlocal || host.starts_with("localhost") || host.starts_with("127.0.0.1") {
        return Ok(());
    }
    anyhow::bail!("refusing non-local registry host `{host}`; use --allow-nonlocal")
}

fn credentials(args: &Args) -> anyhow::Result<RegistryCredentials> {
    if let Some(token) = &args.token {
        return Ok(RegistryCredentials::Bearer {
            token: token.to_string(),
        });
    }
    match (&args.username, &args.password) {
        (Some(username), Some(password)) => Ok(RegistryCredentials::Basic {
            username: username.clone(),
            password: password.clone(),
        }),
        (None, None) => Ok(RegistryCredentials::Anonymous),
        (Some(_), None) => anyhow::bail!("--password is required when --username is set"),
        (None, Some(_)) => anyhow::bail!("--username is required when --password is set"),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    validate_reference_host(&args.reference, args.allow_nonlocal)?;
    validate_manifest_path(&args.manifest).await?;

    let creds = credentials(&args)?;
    let push = push_update(&PushOptions {
        reference: args.reference.clone(),
        manifest_path: args.manifest.clone(),
        changelog_path: args.changelog.clone(),
        credentials: creds.clone(),
    })
    .await?;

    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let output_root = std::env::temp_dir().join(format!("ota-update-smoke-{stamp}"));
    let mut feedback = NoopFeedback;
    let pull = pull_update(
        &PullOptions {
            reference: args.reference,
            destination_root: output_root.clone(),
            credentials: creds,
            install: false,
            validate: true,
        },
        &mut feedback,
    )
    .await?;

    validate_manifest_path(&pull.manifest_path).await?;
    println!(
        "PASS pushed={} pulled={}",
        push.manifest_url,
        pull.output_dir.display()
    );

    if args.cleanup {
        let _ = tokio::fs::remove_dir_all(&output_root).await;
    }
    Ok(())
}
