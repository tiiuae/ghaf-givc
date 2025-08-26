use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Revision {
    pub store_path: PathBuf,
    pub created_on: String,
    pub revision: i32,
    pub artifacts: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pin {
    pub name: String,
    pub created_on: String,
    pub last_revision: Revision,
}

pub type PinList = Vec<Pin>;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheInfo {
    pub name: String,
    pub uri: String,
    pub public_signing_keys: Vec<String>,
    pub permission: String,
    pub preferred_compression_method: String,
    pub github_username: String,
}
