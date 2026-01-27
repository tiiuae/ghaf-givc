mod data;
mod helpers;

use crate::bootctl::parse_bootctl;
use crate::image::manifest::Manifest;
use crate::image::runtime::Runtime;
use data::{BOOTCTL, KERNEL_CMDLINE, LVS, LVS_INSTALLED, MANIFEST};

pub use helpers::{groups, manifest, slots, volume};

pub fn make_test_runtime() -> Runtime {
    let bootctl = parse_bootctl(&BOOTCTL).unwrap();
    Runtime::new(&LVS, "root=fstab", bootctl).unwrap()
}

pub fn make_test_runtime_installed() -> Runtime {
    let bootctl = parse_bootctl(&BOOTCTL).unwrap();
    Runtime::new(&LVS_INSTALLED, &KERNEL_CMDLINE, bootctl).unwrap()
}

pub fn make_test_manifest() -> Manifest {
    serde_json::from_str(&MANIFEST).unwrap()
}
