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
        manifest_version: 0,
        system: None,
        version: version.into(),
        root_verity_hash: hash.into(),
        kernel: File {
            name: "k".into(),
            sha256sum: [0; 32],
        },
        store: File {
            name: "s".into(),
            sha256sum: [0; 32],
        },
        verity: File {
            name: "v".into(),
            sha256sum: [0; 32],
        },
    }
}

pub fn groups(names: &[&str]) -> Vec<SlotGroup> {
    let vols = names.iter().map(|n| Volume::new(n));
    let (slots, _unparsed) = Slot::from_volumes(vols);
    SlotGroup::group_volumes(slots, Vec::new()).unwrap()
}
