use crate::query::query_avaliable_updates;
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
}

/// # Errors
/// Fails if fetch/parse raise failure
pub async fn query_updates(query: QueryUpdates) -> anyhow::Result<()> {
    let updates = query_avaliable_updates(&query.source).await?;
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
