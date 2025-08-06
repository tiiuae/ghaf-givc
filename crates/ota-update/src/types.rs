use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// Fields here should match with output of `nixos-rebuild list-generation --json`
// FIXME: this structure eventually would be merged with `Generations` from sibling PR
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub name: String,
    pub store_path: PathBuf,
    pub current: bool,
    pub pub_key: String,
}

pub struct ProfileElement {
    pub num: i32,
    pub store_path: PathBuf,
    pub current: bool,
}
