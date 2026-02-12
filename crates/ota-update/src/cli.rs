// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use crate::query::query_available_updates;
use crate::types::UpdateInfo;
use clap::Parser;
use serde_json;

#[derive(Parser, Clone, Debug)]
pub struct QueryUpdates {
    #[arg(long)]
    source: String,

    #[arg(long)]
    raw: bool,

    #[arg(long)]
    current: bool,

    #[arg(long, default_value = "ghaf-updates")]
    pin_name: String,
}

#[derive(Parser, Clone, Debug)]
pub struct CachixOptions {
    pub pin_name: String,

    #[arg(long, env = "CACHIX_TOKEN")]
    pub token: Option<String>,

    #[arg(long, default_value = "ghaf-dev")]
    pub cache: String,

    #[arg(long)]
    pub cachix_host: Option<String>,
}

/// # Errors
/// Fails if fetch/parse raise failure
pub async fn query_updates(query: QueryUpdates) -> anyhow::Result<()> {
    let updates = query_available_updates(&query.source, &query.pin_name).await?;
    let iter = updates
        .into_iter()
        .filter(|each| query.current || each.current);

    if query.raw {
        for each in iter {
            println!("{}", each.store_path.display());
        }
    } else {
        let updates: Vec<UpdateInfo> = iter.collect();
        println!("{}", serde_json::to_string(&updates)?);
    }
    Ok(())
}
