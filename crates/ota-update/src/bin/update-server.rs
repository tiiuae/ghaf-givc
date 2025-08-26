use anyhow::Context;
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use cachix_client::types::{CacheInfo, Pin, Revision};
use clap::{Parser, Subcommand};
use ota_update::{profile, types::UpdateInfo};
use std::{ffi::OsString, net::SocketAddr, path::PathBuf, sync::Arc};
use tokio::fs;
use tokio::net::TcpListener;
use tracing::{debug, info, trace};

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Clone, Debug)]
enum Commands {
    Serve(Serve),
    Register {
        path: PathBuf,
        profile: OsString,
        closure: PathBuf,
    },
}

#[derive(Parser, Clone, Debug)]
#[command(author, version, about)]
struct Serve {
    /// Directory containing update symlinks
    #[arg(long, default_value = "/nix/var/nix/profiles/per-user/update")]
    path: PathBuf,

    /// Allowed profile names
    #[clap(long, use_value_delimiter = true)]
    allowed_profiles: Vec<String>,

    /// Port to listen on
    #[arg(long, default_value_t = 3000)]
    port: u16,

    #[arg(long)]
    pub_key: String,

    /// Pretend cachix cache on that URL
    #[arg(long)]
    cachix: String,
}

// Make our own error that wraps `anyhow::Error`.
#[derive(thiserror::Error, Debug)]
#[error("{0}")]
struct Error(#[from] anyhow::Error);

// Tell axum how to convert `AppError` into a response.
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Something went wrong: {}", self.0),
        )
            .into_response()
    }
}

async fn get_update_list(
    path: &std::path::Path,
    default_name: &str,
    pub_key: &str,
) -> Result<Vec<UpdateInfo>, anyhow::Error> {
    info!(
        "Query updates for {path}, default {default_name}",
        path = path.display()
    );

    let (_, profiles) = profile::read_profile_links(path, default_name)
        .await
        .with_context(|| {
            format!(
                "While reading profile {path} with {default_name}",
                path = path.display()
            )
        })?;

    Ok(profiles
        .into_iter()
        .map(|profile| UpdateInfo {
            name: format!("Update #{num}", num = profile.num),
            store_path: profile.store_path,
            current: profile.current,
            pub_key: pub_key.to_owned(),
        })
        .collect())
}

async fn update_handler(
    Path(profile): Path<String>,
    State(serve): State<Arc<Serve>>,
) -> Result<Json<Vec<UpdateInfo>>, Error> {
    trace!("update handler");
    if !serve.allowed_profiles.contains(&profile) {
        info!("Requested profile {profile} not in list of allowed profiles");
        return Ok(Json(vec![])); // or return an error status
    }
    let links = get_update_list(&serve.path, &profile, &serve.pub_key).await?;
    Ok(Json(links))
}

async fn boot_json(
    Path((profile, hash)): Path<(String, String)>,
    State(serve): State<Arc<Serve>>,
) -> axum::response::Response {
    if !serve.allowed_profiles.contains(&profile) {
        info!("Requested profile {profile} not in list of allowed profiles");
        return (StatusCode::NOT_FOUND, "boot.json not found").into_response();
    }

    let Ok(profiles) = get_update_list(&serve.path, &profile, &serve.pub_key).await else {
        info!("Profile {profile} not allowed");
        return (StatusCode::NOT_FOUND, "boot.json not found").into_response();
    };

    let base = profiles.into_iter().find(|p| {
        p.store_path
            .file_name()
            .and_then(|f| f.to_str())
            .is_some_and(|s| s.starts_with(&format!("{hash}-")))
    });
    let base = match base {
        Some(p) => p.store_path,
        None => {
            info!("profile not found for cache={profile}, hash={hash}");
            return (
                StatusCode::NOT_FOUND,
                format!("profile not found for cache={profile}, hash={hash}"),
            )
                .into_response();
        }
    };

    let boot_json = base.join("boot.json");
    debug!(
        "Querying boot.json: {boot_json}",
        boot_json = boot_json.display()
    );

    match fs::read(&boot_json).await {
        Ok(content) => {
            info!("Serving boot.json");
            (
                StatusCode::OK,
                [("Content-Type", "application/json")],
                content,
            )
                .into_response()
        }
        Err(err) => {
            info!(
                "unable to read {boot_json} error {err}",
                boot_json = boot_json.display()
            );
            (
                StatusCode::NOT_FOUND,
                format!(
                    "boot.json not found at {boot_json}",
                    boot_json = boot_json.display()
                ),
            )
                .into_response()
        }
    }
}

async fn cache_info(
    Path(profile): Path<String>,
    State(serve): State<Arc<Serve>>,
) -> Result<Json<CacheInfo>, Error> {
    let info = CacheInfo {
        name: profile,
        uri: serve.cachix.clone(),
        public_signing_keys: vec![serve.pub_key.clone()],
        permission: "read".into(),
        preferred_compression_method: "ZSTD".into(),
        github_username: "bogus".into(),
    };
    Ok(Json(info))
}

async fn pin_list(
    profile: Path<String>,
    serve: State<Arc<Serve>>,
) -> Result<Json<Vec<Pin>>, Error> {
    const CREATED_ON: &str = "dummy date";

    let profile = profile.0;
    let serve = serve.0;
    if !serve.allowed_profiles.contains(&profile) {
        info!("Requested profile {profile} not in list of allowed profiles");
        return Err(Error(anyhow::anyhow!(
            "Requested profile {profile} not in list of allowed profiles"
        )));
    }

    let profiles = get_update_list(&serve.path, &profile, &serve.pub_key).await?;

    let pins = profiles
        .into_iter()
        .map(|each| Pin {
            name: profile.clone(),
            created_on: CREATED_ON.to_owned(),
            last_revision: Revision {
                store_path: each.store_path,
                revision: 1,
                artifacts: Vec::new(),
                created_on: CREATED_ON.to_owned(),
            },
        })
        .collect();
    Ok(Json(pins))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let args = Args::parse();

    match args.command {
        Commands::Serve(serve) => {
            let port = serve.port;
            let state = Arc::new(serve);

            let app = Router::new()
                // Own API
                .route("/update/{profile}", get(update_handler))
                // Cachix API
                .route("/api/v1/cache/{cache}/", get(cache_info))
                .route("/api/v1/cache/{cache}/pin", get(pin_list))
                .route(
                    "/api/v1/cache/{cache}/serve/{hash}/boot.json",
                    get(boot_json),
                )
                .with_state(state);

            let addr = SocketAddr::from(([127, 0, 0, 1], port));
            tracing::info!("Serving on http://{addr}");

            let listener = TcpListener::bind(addr).await?;
            axum::serve(listener, app).await?;
        }
        Commands::Register {
            path,
            profile,
            closure,
        } => profile::set(&path, &profile, &closure).await?,
    }
    Ok(())
}
