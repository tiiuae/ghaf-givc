use crate::types::UpdateInfo;
use anyhow::Context;

/// # Errors
/// Fails if unless fetch/parse update info from server
pub async fn query_avaliable_updates(server_url: &str) -> anyhow::Result<Vec<UpdateInfo>> {
    let url = format!("{server_url}/update");
    let info = reqwest::get(url.clone())
        .await
        .with_context(|| format!("Fail to fetch updates info! Url is {url}"))?
        .json()
        .await
        .context("fail to parse update info json")
}
