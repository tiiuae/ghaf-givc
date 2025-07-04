use crate::{CachixClient, CachixError, Pin};
use bootspec;

/// Filters pins that contain a valid NixOS boot specification (`boot.json`).
///
/// Returns a list of (Pin, BootSpec).
pub async fn filter_valid_systems(
    client: &CachixClient,
    system: &str,
) -> Result<Vec<(Pin, bootspec::v1::GenerationV1)>, CachixError> {
    let pins = client.list_pins().await?;
    let mut result = Vec::new();

    for pin in pins {
        let Some((hash, _)) = pin
            .last_revision
            .store_path
            .strip_prefix("/nix/store")
            .ok()
            .and_then(std::path::Path::file_name)
            .and_then(std::ffi::OsStr::to_str)
            .and_then(|name| name.split_once('-'))
        else {
            continue;
        };

        let Ok(boot_json_bytes) = client.get_file_from_store(hash, "boot.json").await else {
            continue; // skip if not present or error
        };

        if let Ok(spec) = serde_json::from_slice::<bootspec::v1::GenerationV1>(&boot_json_bytes) {
            if spec.bootspec.toplevel.0 == pin.last_revision.store_path
                && spec.bootspec.system == system
            {
                result.push((pin, spec))
            }
        };
    }

    Ok(result)
}
