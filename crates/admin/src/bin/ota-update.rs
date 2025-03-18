use std::fs;
use std::io::BufReader;
use std::process::{Command, Stdio};

use clap::{ArgAction, Parser, Subcommand};
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
        #[arg()]
        path: String,

        /// Source of configuration value
        #[arg(long)]
        source: Option<String>,

        #[arg(long, action = ArgAction::SetTrue, required = false, default_value_t = false)]
        no_check_signs: bool,
    },
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
    for obj in &mut gens {
        if let Value::Object(map) = obj {
            if let Some(gen) = map.get("generation") {
                let gen = gen.as_i64().unwrap();
                let path = format!("/nix/var/nix/profiles/system-{gen}-link");
                let link = fs::read_link(&path)?.to_string_lossy().to_string();
                map.insert("storePath".to_string(), Value::String(link));
            }
        }
    }
    println!("{}", serde_json::to_string(&gens)?);
    Ok(())
}

fn is_valid_nix_path(path: &str) -> anyhow::Result<()> {
    // nix hashes don't contain [eotu]
    let pattern = r"^/nix/store/[a-df-np-sv-z0-9]{32}-nixos-system-[^/]+$";
    let re = Regex::new(pattern).expect("Invalid regex");
    if !re.is_match(path) {
        anyhow::bail!("Path {path} is not valid NixOS system path!")
    }
    Ok(())
}

fn set_generation(
    path: String,
    source: Option<String>,
    no_check_signs: bool,
) -> anyhow::Result<()> {
    is_valid_nix_path(&path)?;
    let from = source
        .as_deref()
        .unwrap_or("https://prod-cache.vedenemo.dev");

    let mut nix = Command::new("nix");
    nix.arg("--extra-experimental-features")
        .arg("nix-command")
        .arg("copy")
        .arg("--from")
        .arg(&from)
        .arg(&path);
    if no_check_signs {
        nix.arg("--no-check-sigs");
    }
    let nix = nix.status().expect("Failed to execute nix copy");
    if !nix.success() {
        anyhow::bail!("nix copy failed")
    }
    let nix_env = Command::new("nix-env")
        .arg("-p")
        .arg("/nix/var/nix/profiles/system")
        .arg("--set")
        .arg(&path)
        .status()
        .expect("Fail to execute nix-env");
    if !nix_env.success() {
        anyhow::bail!("nix-env failed")
    }
    let boot_path = format!("{path}/bin/switch-to-configuration");
    let boot = Command::new(&boot_path)
        .arg("boot")
        .status()
        .expect("Fail to execute switch-to-configuration");
    if !boot.success() {
        anyhow::bail!("switch-to-configuration failed")
    }
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
        } => set_generation(path, source, no_check_signs)?,
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::is_valid_nix_path;
    #[test]
    fn test_validation() -> anyhow::Result<()> {
        let path = format!(
            "/nix/store/{}",
            "b4fmrar918b1l8hwfjzxqv7whnq5c33q-nixos-system-adminvm-test"
        );
        is_valid_nix_path(&path)
    }
}
