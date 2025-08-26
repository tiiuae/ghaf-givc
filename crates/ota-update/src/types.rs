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

/// This structure more or less matched output of nixos-rebuild list-generations --json
/// but extended with some extra information
///
/// This intended to pass from `ota-update` to `admin` service in jsoned form
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools)]
pub struct GenerationDetails {
    pub name: String,
    pub generation: i32,
    pub nixos_version: String,
    pub kernel_version: String,
    pub nixpkgs_revision: Option<String>,
    pub configuration_revision: Option<String>,

    pub store_path: PathBuf,

    // This generation match /run/current-system
    pub current: bool,

    // This generation match /run/booted-system
    pub booted: bool,

    // This generation is next boot candidate in bootctl output
    pub bootable: bool,

    // This generation match /nix/var/nix/profiles/system default
    pub default: bool,

    // Raw bootspec data
    pub bootspec: bootspec::v1::GenerationV1,

    // Raw bootctl info, if generation have matching bootctl record
    pub bootctl: Option<crate::bootctl::BootctlItem>,
}
