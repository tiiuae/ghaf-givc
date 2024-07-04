use clap::{Parser, Subcommand};
use givc::admin::client::AdminClient;
use givc::endpoint::{EndpointConfig, TlsConfig};
use givc::pb;
use givc::types::*;
use givc::utils::naming::*;
use std::net::SocketAddr;
use std::path::PathBuf;
use tracing::info;

#[derive(Debug, Parser)] // requires `derive` feature
#[command(name = "givc-cli")]
#[command(about = "A givc CLI application", long_about = None)]
struct Cli {
    #[arg(long, env = "ADDR", default_missing_value = "127.0.0.1")]
    addr: String,
    #[arg(long, env = "PORT", default_missing_value = "9000")]
    port: u16,

    #[arg(long, env = "HOST_KEY")]
    host_key: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Start {
        app: String,
    },
    Stop {
        app: String,
    },
    Pause {
        app: String,
    },
    Resume {
        app: String,
    },
    Query {
        as_json: bool,        // Would it useful for scripts?
        by_type: Option<u32>, // FIXME:  parse UnitType by names?
        by_name: Vec<String>, // list of names, all if empty?
    },
    Watch {
        as_json: bool,
    },
}

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    givc::trace_init();

    let cli = Cli::parse();
    info!("CLI is {:#?}", cli);

    let addr = SocketAddr::new(cli.addr.parse()?, cli.port);

    let admin_cfg = EndpointConfig {
        transport: TransportConfig {
            address: cli.addr,
            port: cli.port,
            protocol: "bogus".into(),
        },
        tls: None, // No TLS in cli at the moment
    };

    let admin = AdminClient::new(admin_cfg);

    match cli.command {
        Commands::Start { app } => admin.start(app).await?,
        Commands::Stop { app } => admin.stop(app).await?,

        Commands::Pause { app } => admin.pause(app).await?,
        Commands::Resume { app } => admin.resume(app).await?,

        Commands::Query {
            by_type,
            by_name,
            as_json,
        } => {
            let ty = match by_type {
                Some(x) => Some(UnitType::try_from(x)?),
                None => None,
            };
            let reply = admin.query(ty, by_name).await?;
            if as_json {
                let js = serde_json::to_string(&reply)?;
                println!("{}", js);
            } else {
                println!("{:#?}", reply);
            }
        }
        Commands::Watch { as_json } => {
            admin
                .watch(|event| {
                    if as_json {
                        let js = serde_json::to_string(&event)?;
                        println!("{}", js)
                    } else {
                        println!("{:#?}", event)
                    };
                    Ok(())
                })
                .await?
        }
    };

    Ok(())
}
