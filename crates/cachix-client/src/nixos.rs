use crate::{CachixClient, CachixError, Pin};
use bootspec;

/// Filters pins that contain a valid NixOS boot specification (`boot.json`).
///
/// Returns a list of (Pin, BootSpec).
pub async fn filter_valid_systems(
    client: &CachixClient,
) -> Result<Vec<(Pin, bootspec::v1::GenerationV1)>, CachixError> {
    let pins = client.list_pins().await?;
    let mut result = Vec::new();

    for pin in pins {
        // Expecting store_path like: /nix/store/<hash>-something
        let Ok(path) = pin.last_revision.store_path.strip_prefix("/nix/store") else {
            continue;
        };

        // Expecting store_path like: /nix/store/<hash>-something
        let Some(name) = path.file_name().map(|s| s.to_string_lossy()) else {
            continue;
        };
        let Some(hash) = name.split('-').next() else {
            continue;
        };

        let boot_json_bytes = match client.get_file_from_store(hash, "boot.json").await {
            Ok(bytes) => bytes,
            Err(_) => continue, // skip if not present or error
        };

        if let Ok(spec) = serde_json::from_slice::<bootspec::v1::GenerationV1>(&boot_json_bytes) {
            if spec.bootspec.toplevel.0 == pin.last_revision.store_path {
                result.push((pin, spec))
            }
        };
    }

    Ok(result)
}
