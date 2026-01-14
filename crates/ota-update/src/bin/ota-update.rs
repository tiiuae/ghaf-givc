use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use tokio::process::Command;

use anyhow::Context;
use cachix_client::{CachixClientConfig, nixos::filter_valid_systems};
use clap::{ArgAction, Parser, Subcommand};
use ota_update::cli::{CachixOptions, QueryUpdates, query_updates};
use ota_update::image::cli::ImageUpdate;
use ota_update::profile;
use ota_update::query::query_available_updates;
use regex::Regex;
use tracing::info;

#[derive(Parser, Debug)]
#[command(
    name = "ota-update",
    about = "A tool to get or set generations",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Retrieve the configuration value
    Get,

    /// Set the configuration value
    Local {
        path: Option<PathBuf>,

        /// Source of configuration value
        #[arg(long, default_value = "https://prod-cache.vedenemo.dev")]
        source: String,

        #[arg(long, action = ArgAction::SetTrue, required = false, default_value_t = false)]
        no_check_signs: bool,

        #[arg(long, default_value = "ghaf-updates")]
        pin_name: String,
    },

    /// Query updates list
    Query(QueryUpdates),

    Cachix(CachixOptions),
    Image(ImageUpdate),
}

async fn get_generations() -> anyhow::Result<()> {
    let gens = profile::read_generations()
        .await
        .context("While read list of generations")?;
    println!("{}", serde_json::to_string_pretty(&gens)?);
    Ok(())
}

fn is_valid_nix_path(path: &Path) -> anyhow::Result<()> {
    let path = path
        .to_str()
        .with_context(|| format!("unable to convert `{}` to UTF-8", path.display()))?;
    // nix hashes don't contain [eotu]
    let pattern = r"^/nix/store/[a-df-np-sv-z0-9]{32}-nixos-system-[^/]+$";
    let re = Regex::new(pattern).expect("Invalid regex");
    if !re.is_match(path) {
        anyhow::bail!("Path {path} is not valid NixOS system path!")
    }
    Ok(())
}

async fn set_generation(
    path: &Path,
    sources: &[String],
    pub_keys: &[String],
    no_check_signs: bool,
) -> anyhow::Result<()> {
    const GCROOT: &str = "/nix/var/nix/gcroots/auto/ota-update";

    is_valid_nix_path(path)?;

    let mut nix = Command::new("nix");
    nix.arg("--extra-experimental-features")
        .arg("nix-command")
        .arg("build")
        .arg(path)
        // Protect downloading derivation from GC if it run concurrently.
        // Also workaround bug -- `nix build` attempt symlink ./result in current directory, which could be non-writeable
        .arg("--out-link")
        .arg(GCROOT);
    for source in sources {
        nix.arg("--extra-substituters");
        nix.arg(source);
    }
    for pub_key in pub_keys {
        nix.arg("--extra-trusted-public-keys");
        nix.arg(pub_key);
    }
    if no_check_signs {
        nix.arg("--no-check-sigs");
    }
    let nix = nix
        .status()
        .await
        .context("Failed to execute 'nix build'")?;
    if !nix.success() {
        anyhow::bail!("nix build failed");
    }

    profile::set(
        Path::new("/nix/var/nix/profiles/"),
        OsStr::new("system"),
        path,
    )
    .await?;

    if let Err(e) = tokio::fs::remove_file(GCROOT).await {
        info!("Fail to unlink {GCROOT}: {e}");
    }

    let boot_path = path.join("bin/switch-to-configuration");
    Command::new(&boot_path)
        .arg("boot")
        .status()
        .await
        .with_context(|| format!("Fail to execute {}", boot_path.display()))?;
    Ok(())
}

async fn read_system_boot_json() -> anyhow::Result<String> {
    let contents = tokio::fs::read_to_string("/run/current-system/boot.json").await?;
    let boot_json = serde_json::from_str::<bootspec::v1::GenerationV1>(&contents)?;
    Ok(boot_json.bootspec.system)
}

async fn perform_cachix_update(
    pin_name: &str,
    token: Option<String>,
    host: Option<String>,
    cache: String,
) -> anyhow::Result<()> {
    let system = read_system_boot_json().await?;
    let mut client_config = CachixClientConfig::new(cache);
    if let Some(token) = token {
        client_config = client_config.set_auth_token(token);
    }
    if let Some(host) = host {
        client_config = client_config.set_hostname(host);
    }
    let client = client_config.build();
    let candidate = filter_valid_systems(&client, &system)
        .await?
        .into_iter()
        .find_map(|(pin, _)| (pin.name == pin_name).then_some(pin.last_revision.store_path))
        .context("no valid systems")?;
    let info = client.cache_info().await?;
    set_generation(&candidate, &[info.uri], &info.public_signing_keys, false).await?;
    Ok(())
}

async fn perform_local_update(
    maybe_path: Option<PathBuf>,
    source: String,
    pin_name: String,
    no_check_signs: bool,
) -> anyhow::Result<()> {
    let updates = query_available_updates(&source, &pin_name).await?;
    let candidate = updates
        .into_iter()
        .find(|update| match &maybe_path {
            Some(path) => path == &update.store_path,
            None => update.current,
        })
        .context("No valid candidate found")?;
    set_generation(
        &candidate.store_path,
        &[source],
        &[candidate.pub_key],
        no_check_signs,
    )
    .await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Get => get_generations().await?,
        Commands::Local {
            path,
            source,
            no_check_signs,
            pin_name,
        } => perform_local_update(path, source, pin_name, no_check_signs).await?,
        Commands::Query(query) => {
            query_updates(query).await?;
        }
        Commands::Cachix(CachixOptions {
            pin_name,
            token,
            cachix_host,
            cache,
        }) => perform_cachix_update(&pin_name, token, cachix_host, cache).await?,
        Commands::Image(image) => image.handle().await?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::is_valid_nix_path;
    use std::path::Path;
    #[test]
    fn test_validation() -> anyhow::Result<()> {
        let path = Path::new("/nix/store")
            .join("b4fmrar918b1l8hwfjzxqv7whnq5c33q-nixos-system-adminvm-test");
        is_valid_nix_path(&path)?;
        let path = Path::new("/nix/store").join("../dive/out/of/nix/store");
        assert!(is_valid_nix_path(&path).is_err());
        Ok(())
    }
}
