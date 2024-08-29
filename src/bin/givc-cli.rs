use clap::{Parser, Subcommand};
use givc::endpoint::TlsConfig;
use givc::types::*;
use givc_client::AdminClient;
use serde::ser::Serialize;
use std::path::PathBuf;
use std::time;
use tokio::time::sleep;
use tracing::info;

#[derive(Debug, Parser)] // requires `derive` feature
#[command(name = "givc-cli")]
#[command(about = "A givc CLI application", long_about = None)]
struct Cli {
    #[arg(long, env = "ADDR", default_missing_value = "127.0.0.1")]
    addr: String,
    #[arg(long, env = "PORT", default_missing_value = "9000", value_parser = clap::value_parser!(u16).range(1..))]
    port: u16,

    #[arg(long, env = "HOST_KEY")]
    host_key: Option<PathBuf>,

    #[arg(long, env = "NAME", default_missing_value = "admin.ghaf")]
    name: String, // for TLS service name

    #[arg(long, env = "CA_CERT")]
    cacert: Option<PathBuf>,

    #[arg(long, env = "HOST_CERT")]
    cert: Option<PathBuf>,

    #[arg(long, env = "HOST_KEY")]
    key: Option<PathBuf>,

    #[arg(long, default_value_t = false)]
    notls: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Start {
        #[arg(long)]
        vm: Option<String>,
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
    Reboot {},
    Poweroff {},
    Query {
        #[arg(long, default_value_t = false)]
        as_json: bool, // Would it useful for scripts?
        #[arg(long)]
        by_type: Option<u32>, // FIXME:  parse UnitType by names?
        #[arg(long)]
        by_name: Vec<String>, // list of names, all if empty?
    },
    QueryList {
        // Even if I believe that QueryList is temporary
        #[arg(long, default_value_t = false)]
        as_json: bool,
    },
    Watch {
        #[arg(long, default_value_t = false)]
        as_json: bool,
        #[arg(long, default_value_t = false)]
        initial: bool,
        #[arg(long)]
        limit: Option<u32>,
    },
    Test {
        #[command(subcommand)]
        test: Test,
    },
}

#[derive(Debug, Subcommand)]
enum Test {
    Ensure {
        #[arg(long, default_missing_value = "1")]
        retry: i32,
        service: String,
    },
}

async fn test_subcommands(test: Test, admin: AdminClient) -> anyhow::Result<()> {
    match test {
        Test::Ensure { service, retry } => {
            for _ in 0..retry {
                let reply = admin.query_list().await?;
                if reply.iter().any(|r| r.name == service) {
                    return Ok(());
                }
                sleep(time::Duration::from_secs(1)).await
            }
            anyhow::bail!("test failed '{service}' not registered")
        }
    }
}

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    givc::trace_init()?;

    let cli = Cli::parse();
    info!("CLI is {:#?}", cli);

    let tls = if cli.notls {
        None
    } else {
        Some((
            cli.name.clone(),
            TlsConfig {
                ca_cert_file_path: cli.cacert.expect("cacert is required"),
                cert_file_path: cli.cert.expect("cert is required"),
                key_file_path: cli.key.expect("key is required"),
                tls_name: Some(cli.name),
            },
        ))
    };
    let admin = AdminClient::new(cli.addr, cli.port, tls);

    match cli.command {
        Commands::Test { test } => test_subcommands(test, admin).await?,
        Commands::Start { app, vm } => admin.start(app, vm).await?,
        Commands::Stop { app } => admin.stop(app).await?,
        Commands::Pause { app } => admin.pause(app).await?,
        Commands::Resume { app } => admin.resume(app).await?,
        Commands::Reboot {} => admin.reboot().await?,
        Commands::Poweroff {} => admin.poweroff().await?,

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
            dump(&reply, as_json)?
        }
        Commands::QueryList { as_json } => {
            let reply = admin.query_list().await?;
            dump(&reply, as_json)?
        }
        Commands::Watch {
            as_json,
            limit,
            initial,
        } => {
            let watch = admin.watch().await?;
            let mut limit = limit;

            if initial {
                dump(watch.initial.clone(), as_json)?
            }

            loop {
                let event = watch.channel.recv().await?;
                dump(event, as_json)?;
                if limit.as_mut().is_some_and(|l| {
                    *l -= 1;
                    *l == 0
                }) {
                    break;
                }
            }
        }
    };

    Ok(())
}

fn dump<Q>(qr: Q, as_json: bool) -> anyhow::Result<()>
where
    Q: std::fmt::Debug + Serialize,
{
    if as_json {
        let js = serde_json::to_string(&qr)?;
        println!("{}", js)
    } else {
        println!("{:#?}", qr)
    };
    Ok(())
}
