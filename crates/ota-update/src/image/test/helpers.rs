use crate::image::lvm::Volume;
use crate::image::manifest::{File, Manifest};
use crate::image::slot::Slot;

pub fn volume(name: &str) -> Volume {
    Volume {
        lv_name: name.to_string(),
        vg_name: "vg".into(),
        lv_attr: None,
        lv_size_bytes: None,
    }
}

pub fn slots(names: &[&str]) -> Vec<Slot> {
    let vols: Vec<_> = names.iter().map(|n| volume(n)).collect();
    let (slots, _unparsed) = Slot::from_volumes(vols);
    slots
}

pub fn manifest(version: &str, hash: &str) -> Manifest {
    Manifest {
        meta: Default::default(),
        version: version.into(),
        root_verity_hash: hash.into(),
        kernel: File {
            name: "k".into(),
            sha256sum: "x".into(),
        },
        store: File {
            name: "s".into(),
            sha256sum: "x".into(),
        },
        verity: File {
            name: "v".into(),
            sha256sum: "x".into(),
        },
    }
}
