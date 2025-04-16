use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use clap::Parser;
use serde::Serialize;
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::net::TcpListener;
use tokio::{fs, io};

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// Directory containing update symlinks
    #[arg(long, default_value = "/nix/var/nix/profiles/per-user/update")]
    path: PathBuf,

    /// Basename of the default symlink
    #[arg(long, default_value = "update")]
    default_name: String,

    /// Port to listen on
    #[arg(long, default_value_t = 3000)]
    port: u16,
}

#[derive(Clone)]
struct AppState {
    path: PathBuf,
    default_name: String,
}

#[derive(Serialize)]
struct LinkInfo {
    name: String,
    target: String,
    default: bool,
}

// Make our own error that wraps `anyhow::Error`.
struct AppError(anyhow::Error);

// Tell axum how to convert `AppError` into a response.
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Something went wrong: {}", self.0),
        )
            .into_response()
    }
}

// This enables using `?` on functions that return `Result<_, anyhow::Error>` to turn them into
// `Result<_, AppError>`. That way you don't need to do that manually.
impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

async fn get_update_list(
    path: &PathBuf,
    default_name: &str,
) -> Result<Vec<LinkInfo>, anyhow::Error> {
    let default_link_path = path.to_owned().join(&default_name);
    let default_target = fs::read_link(&default_link_path).await.ok();

    let mut updates = Vec::new();
    let mut dir = fs::read_dir(&path).await?;

    while let Some(entry) = dir.next_entry().await? {
        let file_name = match entry.file_name().into_string() {
            Ok(n) => n,
            Err(_) => continue,
        };

        if file_name == default_name {
            continue;
        }

        let full_path = entry.path();

        let target_path = match fs::read_link(&full_path).await {
            Ok(t) => {
                let absolute = if t.is_absolute() {
                    t
                } else {
                    full_path.parent().unwrap_or(Path::new("/")).join(t)
                };

                if !absolute.exists() {
                    continue;
                }

                absolute
            }
            Err(_) => continue,
        };

        let is_default = match &default_target {
            Some(def) => def == Path::new(&file_name),
            None => false,
        };

        updates.push(LinkInfo {
            name: file_name,
            target: target_path.to_string_lossy().into_owned(),
            default: is_default,
        });
    }

    Ok(updates)
}

async fn update_handler(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<LinkInfo>>, AppError> {
    let links = get_update_list(&state.path, &state.default_name).await?;
    Ok(Json(links))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let args = Args::parse();
    let state = Arc::new(AppState {
        path: args.path,
        default_name: args.default_name,
    });

    let app = Router::new()
        .route("/update", get(update_handler))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], args.port));
    tracing::info!("Serving on http://{}", addr);

    let listener = TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
