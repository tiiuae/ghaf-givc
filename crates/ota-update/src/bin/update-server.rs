use anyhow::Context;
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use clap::{Parser, Subcommand};
use ota_update::{profile, types::UpdateInfo};
use std::{
    ffi::OsString,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::net::TcpListener;
use tracing::{info, trace};

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
    path: &Path,
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
    axum::extract::Path(profile): axum::extract::Path<String>,
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
                .route("/update/{profile}", get(update_handler))
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
