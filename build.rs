use std::env;
use std::path::PathBuf;

fn main() {
    let proto = "api/admin/admin.proto";

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    tonic_build::configure()
        .file_descriptor_set_path(out_dir.join("admin_descriptor.bin"))
        .compile(&["api/admin/admin.proto"], &["admin"])
        .unwrap();
}
