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
use tokio::fs;
use tokio::net::TcpListener;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
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

        if file_name == default_name
            || !file_name.starts_with(default_name)
            || !file_name.ends_with("-link")
        {
            continue;
        }

        let full_path = entry.path();

        let target_path = match fs::read_link(&full_path).await {
            Ok(t) if t.is_absolute() && t.exists() => t,
            Ok(_) => continue,
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
    axum::extract::Path(profile): axum::extract::Path<String>,
    State(args): State<Arc<Args>>,
) -> Result<Json<Vec<LinkInfo>>, AppError> {
    if !args.allowed_profiles.contains(&profile) {
        return Ok(Json(vec![])); // or return an error status
    }
    let links = get_update_list(&args.path, &profile).await?;
    Ok(Json(links))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let args = Args::parse();
    let port = args.port;
    let state = Arc::new(args);

    let app = Router::new()
        .route("/update/:profile", get(update_handler))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    tracing::info!("Serving on http://{}", addr);

    let listener = TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
