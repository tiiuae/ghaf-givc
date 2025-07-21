use crate::types::UpdateInfo;
use anyhow::Context;

/// # Errors
/// Fails if unless fetch/parse update info from server
pub async fn query_available_updates(
    server_url: &str,
    pin_name: &str,
) -> anyhow::Result<Vec<UpdateInfo>> {
    let url = format!("{server_url}/update/{pin_name}");
    reqwest::get(&url)
        .await
        .with_context(|| format!("Fail to fetch updates info! Url is {url}"))?
        .json()
        .await
        .context("fail to parse update info json")
}
