use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

use clap::{ArgAction, Parser};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Parser, Debug)]
#[command(
    name = "ota-update",
    about = "A tool to get or set generations",
    version
)]
struct Cli {
    /// Retrieve the configuration value
    #[arg(long, conflicts_with = "set", action = ArgAction::SetTrue)]
    get: bool,

    /// Set the configuration value
    #[arg(long, conflicts_with = "get")]
    set: Option<String>,
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
    let pattern = r"^/nix/store/[a-z0-9]{32}-nixos-system-ghaf-host-[0-9]{2}\.[0-9]{2}\.[0-9]{8}\.[a-f0-9]{7}$";
    let re = Regex::new(pattern).expect("Invalid regex");
    if !re.is_match(path) {
        anyhow::bail!("Path {path} is not valid ghaf!")
    }
    Ok(())
}

fn set_generation(path: String) -> anyhow::Result<()> {
    is_valid_nix_path(&path)?;

    let nix = Command::new("nix")
        .arg("copy")
        .arg("--from")
        .arg("https://prod-cache.vedenemo.dev")
        .arg(&path)
        .status()
        .expect("Failed to execute nix copy");
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
    let boot_path = format!("{path}//bin/switch-to-configuration");
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

    if cli.get {
        get_generations()?
    } else if let Some(path) = cli.set {
        set_generation(path)?
    } else {
        eprintln!("Either --get or --set <path> must be specified.")
    };
    Ok(())
}
