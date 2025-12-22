use std::env;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    for pkg in [
        "admin",
        "exec",
        "locale",
        "systemd",
        "stats",
        "notify",
        "policyagent",
    ] {
        tonic_prost_build::configure()
            .file_descriptor_set_path(out_dir.join(format!("{pkg}_descriptor.bin")))
            .type_attribute(
                ".admin.Generation",
                "#[derive(Deserialize, Serialize)] #[serde(rename_all = \"camelCase\")]",
            )
            .compile_protos(&[format!("api/{pkg}/{pkg}.proto").as_str()], &["api"])?;
    }

    Ok(())
}
