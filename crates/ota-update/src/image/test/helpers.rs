// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use crate::image::group::SlotGroup;
use crate::image::lvm::Volume;
use crate::image::manifest::{File, Manifest};
use crate::image::slot::Slot;

pub fn slots(names: &[&str]) -> Vec<Slot> {
    let vols = names.iter().map(|n| Volume::new(n));
    let (slots, _unparsed) = Slot::from_volumes(vols);
    slots
}

pub fn manifest(version: &str, hash: &str) -> Manifest {
    Manifest {
        meta: Default::default(),
        manifest_version: 2,
        target: "test-target".into(),
        generation: 2,
        version: version.into(),
        root_verity_hash: hash.into(),
        kernel: File {
            name: "k".into(),
            sha256sum: [0; 32],
            packed_size: 1,
            unpacked_size: 1,
        },
        store: File {
            name: "s".into(),
            sha256sum: [0; 32],
            packed_size: 1,
            unpacked_size: 6_000_000_000, // ~5.6 GiB
        },
        verity: File {
            name: "v".into(),
            sha256sum: [0; 32],
            packed_size: 1,
            unpacked_size: 60_000_000, // ~57 MiB
        },
    }
}

pub fn groups(names: &[&str]) -> Vec<SlotGroup> {
    let vols = names.iter().map(|n| Volume::new(n));
    let (slots, _unparsed) = Slot::from_volumes(vols);
    SlotGroup::group_volumes(slots, Vec::new()).unwrap()
}
