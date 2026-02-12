// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use cachix_client::CachixClientConfig;
use clap::Parser;
use std::io::{self, Write};

/// Simple CLI for testing Cachix API
#[derive(Parser)]
#[command(name = "cachix-cli")]
#[command(version, about = "Interact with Cachix pins", long_about = None)]
struct Cli {
    /// Cachix cache name
    #[arg(short, long, default_value = "ghaf-untrusted")]
    cache: String,

    /// Auth token (or use CACHIX_AUTH_TOKEN env var)
    #[arg(env = "CACHIX_AUTH_TOKEN", long)]
    token: Option<String>,

    /// Host to connect
    #[arg(long)]
    host: Option<String>,

    /// Command to run
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Info about the cache
    Info,

    /// List all pins in the cache
    ListPins,

    /// Delete a pin by name
    DeletePin {
        /// Name of the pin to delete
        name: String,
    },

    /// Fetch file from a pinned store path via serve
    Serve {
        /// Store path hash (narHash)
        hash: String,

        /// File path inside store path
        path: String,
    },

    #[cfg(feature = "nixos")]
    ListSystems {
        #[arg(long, default_value = "x86_64-linux")]
        system: String,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let mut client_config = CachixClientConfig::new(cli.cache);
    if let Some(token) = cli.token {
        client_config = client_config.set_auth_token(token)
    }
    if let Some(host) = cli.host {
        client_config = client_config.set_hostname(host)
    }
    let client = client_config.build();

    match cli.command {
        Commands::Info => {
            let info = client.cache_info().await?;
            println!("{info:?}")
        }
        Commands::ListPins => {
            let pins = client.list_pins().await?;
            for pin in pins {
                println!("{} -> {}", pin.name, pin.last_revision.store_path.display());
            }
        }
        Commands::DeletePin { name } => {
            client.delete_pin(&name).await?;
            println!("Deleted pin: {}", name);
        }
        Commands::Serve { hash, path } => {
            let data = client.get_file_from_store(&hash, &path).await?;
            let mut stdout = io::stdout().lock();
            stdout.write_all(&data)?;
            stdout.flush()?;
        }

        #[cfg(feature = "nixos")]
        Commands::ListSystems { system } => {
            let systems = cachix_client::nixos::filter_valid_systems(&client, &system).await?;
            for (pin, spec) in systems {
                println!(
                    "{} -> {} ({})",
                    pin.name,
                    pin.last_revision.store_path.display(),
                    spec.bootspec.label,
                );
            }
        }
    }

    Ok(())
}
