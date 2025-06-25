use cachix_client::CachixClient;
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

    /// Command to run
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
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
    ListSystems,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let client = CachixClient::new(cli.cache, cli.token);

    match cli.command {
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
        Commands::ListSystems => {
            let systems = cachix_client::nixos::filter_valid_systems(&client).await?;
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
