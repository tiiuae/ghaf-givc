use clap::{Parser, Subcommand};
use givc::endpoint::TlsConfig;
use givc::types::*;
use givc::utils::vsock::parse_vsock_addr;
use givc_client::client::AdminClient;
use givc_common::address::EndpointAddress;
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

    #[arg(long)]
    vsock: Option<String>,

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
enum StartSub {
    App {
        app: String,
        #[arg(long)]
        vm: String,
        #[arg(last = true)]
        args: Vec<String>,
    },
    Vm {
        vm: String,
    },
    Service {
        servicename: String,
        #[arg(long)]
        vm: String,
    },
}

#[derive(Debug, Subcommand)]
enum Commands {
    Start {
        #[command(subcommand)]
        start: StartSub,
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
    Suspend {},
    Wakeup {},
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
    SetLocale {
        locale: String,
    },
    SetTimezone {
        timezone: String,
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

    // FIXME; big kludge, but allow to test vsock connection
    let admin = if let Some(vsock) = cli.vsock {
        info!("Connection diverted to VSock");
        AdminClient::from_endpoint_address(EndpointAddress::Vsock(parse_vsock_addr(&vsock)?), tls)
    } else {
        AdminClient::new(cli.addr, cli.port, tls)
    };

    match cli.command {
        Commands::Test { test } => test_subcommands(test, admin).await?,
        Commands::Start { start } => {
            let response = match start {
                StartSub::App { app, vm, args } => admin.start_app(app, vm, args).await?,
                StartSub::Vm { vm } => admin.start_vm(vm).await?,
                StartSub::Service { servicename, vm } => {
                    admin.start_service(servicename, vm).await?
                }
            };
            println!("{:?}", response)
        }
        Commands::Stop { app } => admin.stop(app).await?,
        Commands::Pause { app } => admin.pause(app).await?,
        Commands::Resume { app } => admin.resume(app).await?,
        Commands::Reboot {} => admin.reboot().await?,
        Commands::Poweroff {} => admin.poweroff().await?,
        Commands::Suspend {} => admin.suspend().await?,
        Commands::Wakeup {} => admin.wakeup().await?,

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
            dump(reply, as_json)?
        }
        Commands::QueryList { as_json } => {
            let reply = admin.query_list().await?;
            dump(reply, as_json)?
        }

        Commands::SetLocale { locale } => {
            admin.set_locale(locale).await?;
        }

        Commands::SetTimezone { timezone } => {
            admin.set_timezone(timezone).await?;
        }

        Commands::Watch {
            as_json,
            limit,
            initial: dump_initial,
        } => {
            let watch = admin.watch().await?;
            let mut limit = limit.map(|l| 0..l);

            if dump_initial {
                dump(watch.initial, as_json)?
            }

            // Change to Option::is_none_or() with rust >1.82
            while !limit.as_mut().is_some_and(|l| l.next().is_none()) {
                dump(watch.channel.recv().await?, as_json)?;
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
