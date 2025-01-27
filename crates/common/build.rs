use std::env;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    for pkg in ["admin", "locale", "systemd", "stats", "stats_message"] {
        tonic_build::configure()
            .file_descriptor_set_path(out_dir.join(format!("{pkg}_descriptor.bin")))
            .compile_protos(&[&format!("api/{pkg}/{pkg}.proto")], &["api"])?;
    }

    Ok(())
}
