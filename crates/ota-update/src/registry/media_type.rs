// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumString};

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Hash, AsRefStr, Display, EnumString, Serialize, Deserialize,
)]
pub enum MediaType {
    #[strum(serialize = "application/vnd.ghaf.ota.manifest.v1+json")]
    #[serde(rename = "application/vnd.ghaf.ota.manifest.v1+json")]
    Manifest,
    #[strum(serialize = "application/vnd.ghaf.ota.uki.v1+efi")]
    #[serde(rename = "application/vnd.ghaf.ota.uki.v1+efi")]
    Uki,
    #[strum(serialize = "application/vnd.ghaf.ota.root.v1+raw")]
    #[serde(rename = "application/vnd.ghaf.ota.root.v1+raw")]
    Root,
    #[strum(serialize = "application/vnd.ghaf.ota.verity.v1+raw")]
    #[serde(rename = "application/vnd.ghaf.ota.verity.v1+raw")]
    Verity,
    #[strum(serialize = "application/vnd.ghaf.ota.changelog.v1+plain")]
    #[serde(rename = "application/vnd.ghaf.ota.changelog.v1+plain")]
    Changelog,
}
