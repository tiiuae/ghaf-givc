use std::env;
use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    tonic_build::configure()
        .file_descriptor_set_path(out_dir.join("admin_descriptor.bin"))
        .compile(&["api/admin/admin.proto"], &["admin"])
        .unwrap();

    tonic_build::configure()
        .file_descriptor_set_path(out_dir.join("locale_descriptor.bin"))
        .compile(&["api/locale/locale.proto"], &["locale"])
        .unwrap();

    tonic_build::configure()
        .file_descriptor_set_path(out_dir.join("systemd_descriptor.bin"))
        .compile(&["api/systemd/systemd.proto"], &["systemd"])
        .unwrap();
}
