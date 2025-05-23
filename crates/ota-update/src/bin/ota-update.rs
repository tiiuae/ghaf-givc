use std::ffi::OsStr;
use std::fs;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::Context;
use clap::{ArgAction, Parser, Subcommand};
use ota_update::cli::{QueryUpdates, query_updates};
use ota_update::profile;
use regex::Regex;
use serde_json::Value;

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
    Set {
        path: PathBuf,

        /// Source of configuration value
        #[arg(long, default_value = "https://prod-cache.vedenemo.dev")]
        source: String,

        #[arg(long, action = ArgAction::SetTrue, required = false, default_value_t = false)]
        no_check_signs: bool,
    },

    /// Query updates list
    Query(QueryUpdates),
}

fn get_generations() -> anyhow::Result<()> {
    let mut nixos_rebuild = Command::new("nixos-rebuild")
        .arg("list-generations")
        .arg("--json")
        .stdout(Stdio::piped())
        .spawn()?;
    // Ensure we can read from stdout
    let stdout = nixos_rebuild
        .stdout
        .take()
        .expect("Failed to capture stdout");
    let reader = BufReader::new(stdout);
    let mut gens: Vec<Value> = serde_json::from_reader(reader)?;
    for map in gens.iter_mut().filter_map(Value::as_object_mut) {
        if let Some(generation) = map.get("generation").and_then(Value::as_i64) {
            let path = format!("/nix/var/nix/profiles/system-{generation}-link");
            let link = fs::read_link(&path)?.to_string_lossy().to_string();
            map.insert("storePath".to_string(), Value::String(link));
        }
    }
    println!("{}", serde_json::to_string(&gens)?);
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

fn set_generation(path: &Path, source: &str, no_check_signs: bool) -> anyhow::Result<()> {
    is_valid_nix_path(path)?;

    let mut nix = Command::new("nix");
    nix.arg("--extra-experimental-features")
        .arg("nix-command")
        .arg("copy")
        .arg("--from")
        .arg(source)
        .arg(path);
    if no_check_signs {
        nix.arg("--no-check-sigs");
    }
    let nix = nix.status().context("Failed to execute nix copy")?;
    if !nix.success() {
        anyhow::bail!("nix copy failed");
    }

    profile::set(
        Path::new("/nix/var/nix/profiles/"),
        OsStr::new("system"),
        path,
    )?;

    let boot_path = path.join("bin/switch-to-configuration");
    Command::new(&boot_path)
        .arg("boot")
        .status()
        .with_context(|| format!("Fail to execute {}", boot_path.display()))?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Get => get_generations()?,
        Commands::Set {
            path,
            source,
            no_check_signs,
        } => set_generation(&path, &source, no_check_signs)?,
        Commands::Query(query) => {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            rt.block_on(query_updates(query))?;
        }
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
