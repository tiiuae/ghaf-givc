use clap::{Parser, Subcommand};
use givc::endpoint::TlsConfig;
use givc::types::UnitType;
use givc::utils::vsock::parse_vsock_addr;
use givc_client::client::AdminClient;
use givc_common::address::EndpointAddress;
use givc_common::pb;
use lazy_regex::regex;
use ota_update::cli::{CachixOptions, QueryUpdates, query_updates};
use serde::ser::Serialize;
use std::path::PathBuf;
use std::time;
use tokio::time::interval;
use tracing::info;

#[derive(Debug, Parser)] // requires `derive` feature
#[command(name = "givc-cli")]
#[command(about = "A givc CLI application", long_about = None)]
struct Cli {
    #[arg(long, env = "GIVC_ADDR", default_missing_value = "127.0.0.1")]
    addr: String,
    #[arg(long, env = "GIVC_PORT", default_missing_value = "9000", value_parser = clap::value_parser!(u16).range(1..))]
    port: u16,

    #[arg(long, env = "GIVC_NAME", default_missing_value = "admin.ghaf")]
    name: String, // for TLS service name

    #[arg(long)]
    vsock: Option<String>,

    #[arg(long, env = "GIVC_CA_CERT")]
    cacert: Option<PathBuf>,

    #[arg(long, env = "GIVC_HOST_CERT")]
    cert: Option<PathBuf>,

    #[arg(long, env = "GIVC_HOST_KEY")]
    key: Option<PathBuf>,

    #[arg(long, env = "GIVC_NO_TLS", default_value_t = false)]
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
enum UpdateSub {
    Query(QueryUpdates),
    List,
    Cachix(CachixOptions),
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
    GetStatus {
        vm_name: String,
        unit_name: String,
    },

    SetLocale {
        locales: Vec<String>,
    },
    SetTimezone {
        timezone: String,
    },
    GetStats {
        vm_name: String,
    },
    Watch {
        #[arg(long, default_value_t = false)]
        as_json: bool,
        #[arg(long, default_value_t = false)]
        initial: bool,
        #[arg(long)]
        limit: Option<u32>,
    },
    Update {
        #[command(subcommand)]
        update: UpdateSub,
    },
    Test {
        #[command(subcommand)]
        test: Test,
    },
    PolicyQuery {
        query: String,
        #[arg(default_value = "")]
        policy_path: String,
    },
}

fn unit_type_parse(s: &str) -> anyhow::Result<UnitType> {
    s.parse::<u32>()?.try_into()
}

fn parse_locales(locale_assigns: Vec<String>) -> anyhow::Result<Vec<pb::locale::LocaleAssignment>> {
    let validator =
        regex!(r"^(?:C|POSIX|[a-z]{2}(?:_[A-Z]{2})?(?:@[a-zA-Z0-9]+)?)(?:\.[-a-zA-Z0-9]+)?$");

    let Some(first) = locale_assigns.first() else {
        anyhow::bail!("No locale assignments provided");
    };

    if validator.is_match(first) {
        return Ok(vec![pb::locale::LocaleAssignment {
            key: pb::locale::LocaleMacroKey::Lang as i32,
            // Item existence validated earlier, `.unwrap()` is safe
            value: locale_assigns.into_iter().next().unwrap(),
        }]);
    }

    let mut parsed_assigns = Vec::new();
    let mut has_lang = false;

    for assign in &locale_assigns {
        let Some((key, value)) = assign.split_once('=') else {
            anyhow::bail!("Invalid locale assignment format: '{assign}'");
        };
        let Some(key_enum) = pb::locale::LocaleMacroKey::from_str_name(key) else {
            anyhow::bail!("Unknown locale key: '{key}'");
        };

        // Validate value for each key
        if !validator.is_match(value) {
            anyhow::bail!("Invalid locale value in '{assign}'");
        }

        if key_enum == pb::locale::LocaleMacroKey::LcAll {
            // LC_ALL overrides all other locale settings, so we can ignore other keys
            return Ok(vec![pb::locale::LocaleAssignment {
                key: pb::locale::LocaleMacroKey::LcAll as i32,
                value: value.to_string(),
            }]);
        }
        if key_enum == pb::locale::LocaleMacroKey::Lang {
            has_lang = true;
        }
        parsed_assigns.push(pb::locale::LocaleAssignment {
            key: key_enum.into(),
            value: value.to_string(),
        });
    }

    if !has_lang {
        anyhow::bail!("At least one of LANG or LC_ALL assignment is required");
    }

    Ok(parsed_assigns)
}

#[derive(Debug, Subcommand)]
enum Test {
    Ensure {
        #[arg(long, default_missing_value = "1")]
        retry: i32,
        service: String,
        #[arg(long, value_parser=unit_type_parse)]
        r#type: Option<UnitType>,
        #[arg(long)]
        vm: Option<String>,
    },
}

impl Test {
    async fn handle(self, admin: AdminClient) -> anyhow::Result<()> {
        let Test::Ensure {
            service,
            retry,
            r#type,
            vm,
        } = self;

        let mut ival = interval(time::Duration::from_secs(1));
        for _ in 0..retry {
            ival.tick().await;
            if let Some(r) = admin
                .query_list()
                .await?
                .into_iter()
                .find(|r| r.name == service)
            {
                if r#type.is_some_and(|t| t.vm != r.vm_type || t.service != r.service_type) {
                    anyhow::bail!("test failed '{service}' registered but of wrong type");
                } else if vm.is_some() && vm != r.vm_name {
                    anyhow::bail!("test failed '{service}' registered but on wrong VM");
                }
                return Ok(());
            }
        }
        anyhow::bail!("test failed '{service}' not registered");
    }
}

impl UpdateSub {
    async fn handle(self, admin: AdminClient) -> anyhow::Result<()> {
        match self {
            UpdateSub::Query(query) => query_updates(query).await?,
            UpdateSub::List => {
                let response = admin.list_generations().await?;
                println!("{response:?}");
            }
            UpdateSub::Cachix(CachixOptions {
                pin_name,
                cachix_host,
                cache,
                token,
            }) => {
                admin
                    .set_generation_cachix(pin_name, cachix_host, cache, token)
                    .await?;
            }
        }
        Ok(())
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
        Commands::Test { test } => test.handle(admin).await?,
        Commands::Start { start } => {
            let response = match start {
                StartSub::App { app, vm, args } => admin.start_app(app, vm, args).await?,
                StartSub::Vm { vm } => admin.start_vm(vm).await?,
                StartSub::Service { servicename, vm } => {
                    admin.start_service(servicename, vm).await?
                }
            };
            println!("{response:?}");
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
            dump(reply, as_json)?;
        }
        Commands::QueryList { as_json } => {
            let reply = admin.query_list().await?;
            dump(reply, as_json)?;
        }

        Commands::GetStatus { vm_name, unit_name } => {
            let reply = admin.get_status(vm_name, unit_name).await?;
            print!("{reply:?}");
        }

        Commands::SetLocale { locales } => {
            admin.set_locales(parse_locales(locales)?).await?;
        }

        Commands::SetTimezone { timezone } => {
            admin.set_timezone(timezone).await?;
        }

        Commands::GetStats { vm_name } => {
            println!("{:?}", admin.get_stats(vm_name).await?);
        }

        Commands::Watch {
            as_json,
            limit,
            initial: dump_initial,
        } => {
            let watch = admin.watch().await?;
            let mut limit = limit.map(|l| 0..l);

            if dump_initial {
                dump(watch.initial, as_json)?;
            }

            while limit.as_mut().is_none_or(|l| l.next().is_some()) {
                dump(watch.channel.recv().await?, as_json)?;
            }
        }

        Commands::Update { update } => update.handle(admin).await?,

        Commands::PolicyQuery { query, policy_path } => {
            let response = admin.policy_query(query, policy_path).await?;
            println!("{:#?}", response.result)
        }
    }

    Ok(())
}

fn dump<Q>(qr: Q, as_json: bool) -> anyhow::Result<()>
where
    Q: std::fmt::Debug + Serialize,
{
    if as_json {
        let js = serde_json::to_string(&qr)?;
        println!("{js}");
    } else {
        println!("{qr:#?}");
    }
    Ok(())
}
