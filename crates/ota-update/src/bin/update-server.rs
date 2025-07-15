use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use clap::{Parser, Subcommand};
use ota_update::profile;
use serde::Serialize;
use std::{
    ffi::OsString,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::fs;
use tokio::net::TcpListener;

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
}

#[derive(Serialize)]
struct UpdateInfo {
    name: OsString,
    target: PathBuf,
    current: bool,
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

async fn get_update_list(path: &Path, default_name: &str) -> anyhow::Result<Vec<UpdateInfo>> {
    let default_link_path = path.join(default_name);
    let default_target = fs::read_link(&default_link_path).await.ok();

    let mut updates = Vec::new();
    let mut dir = fs::read_dir(&path).await?;

    while let Some(entry) = dir.next_entry().await? {
        let name = entry.file_name();

        if name
            .to_str()
            .and_then(|f| f.strip_suffix(default_name))
            .is_none_or(|f| !f.ends_with("-link"))
        {
            continue;
        }

        let full_path = entry.path();

        let target = match fs::read_link(&full_path).await {
            Ok(t) if t.is_absolute() && t.exists() => t,
            _ => continue,
        };

        let current = match &default_target {
            Some(def) => def.as_os_str() == name,
            None => false,
        };

        updates.push(UpdateInfo {
            name,
            target,
            current,
        });
    }

    Ok(updates)
}

async fn update_handler(
    axum::extract::Path(profile): axum::extract::Path<String>,
    State(serve): State<Arc<Serve>>,
) -> Result<Json<Vec<UpdateInfo>>, Error> {
    if !serve.allowed_profiles.contains(&profile) {
        return Ok(Json(vec![])); // or return an error status
    }
    let links = get_update_list(&serve.path, &profile).await?;
    Ok(Json(links))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    match args.command {
        Commands::Serve(serve) => {
            let port = serve.port;
            let state = Arc::new(serve);

            let app = Router::new()
                .route("/update/:profile", get(update_handler))
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
        } => profile::set(&path, &profile, &closure)?,
    }
    Ok(())
}
