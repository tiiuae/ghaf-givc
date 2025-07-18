use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Revision {
    pub store_path: PathBuf,
    pub created_on: String,
    pub revision: i32,
    pub artifacts: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pin {
    pub name: String,
    pub created_on: String,
    pub last_revision: Revision,
}

pub type PinList = Vec<Pin>;
