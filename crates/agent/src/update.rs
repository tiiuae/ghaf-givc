use std::pin::Pin;

use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_stream::Stream;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

use givc_common::pb::update::{
    self, AvailableUpdate, Generation, ImageInstallRequest, ImageInstallResponse,
    ListGenerationsResponse, RegistryChangelogRequest, RegistryChangelogResponse,
    RegistryCredentials, RegistryDiscoverRequest, RegistryDiscoverResponse, RegistryPullRequest,
    RegistryPullResponse, SetGenerationResponse,
    update_server::{Update, UpdateServer},
};

use crate::config::UpdateConfig;

pub type UpdateServiceServer = UpdateServer<UpdateService>;

#[derive(Clone, Debug)]
pub struct UpdateService {
    config: UpdateConfig,
}

impl UpdateService {
    #[must_use]
    pub const fn new(config: UpdateConfig) -> Self {
        Self { config }
    }
}

#[tonic::async_trait]
impl Update for UpdateService {
    async fn list_generations(
        &self,
        _request: Request<update::Empty>,
    ) -> Result<Response<ListGenerationsResponse>, Status> {
        let output = run_command(["get".to_owned()]).await?;
        let generations: Vec<GenerationDetails> = serde_json::from_slice(&output.stdout)
            .map_err(|err| Status::internal(format!("failed to parse generations: {err}")))?;
        let list = generations.into_iter().map(Generation::from).collect();
        Ok(Response::new(ListGenerationsResponse { list }))
    }

    async fn discover(
        &self,
        request: Request<RegistryDiscoverRequest>,
    ) -> Result<Response<RegistryDiscoverResponse>, Status> {
        let request = request.into_inner();
        let mut args = registry_prefix(request.insecure, request.credentials.as_ref());
        args.extend(["discover".to_owned(), request.reference]);
        let output = run_command(args).await?;
        let list = parse_jsonl_result::<Vec<AvailableUpdateJson>>(&output.stdout)?
            .into_iter()
            .map(Into::into)
            .collect();
        Ok(Response::new(RegistryDiscoverResponse { list }))
    }

    async fn changelog(
        &self,
        request: Request<RegistryChangelogRequest>,
    ) -> Result<Response<RegistryChangelogResponse>, Status> {
        let request = request.into_inner();
        let mut args = registry_prefix(request.insecure, request.credentials.as_ref());
        args.extend(["changelog".to_owned(), request.reference]);
        let output = run_command(args).await?;
        let changelog = output
            .stdout
            .split(|byte| *byte == b'\n')
            .filter(|line| {
                serde_json::from_slice::<serde_json::Value>(line)
                    .ok()
                    .and_then(|value| value.get("event").cloned())
                    .is_none()
            })
            .map(String::from_utf8_lossy)
            .collect::<Vec<_>>()
            .join("\n");
        Ok(Response::new(RegistryChangelogResponse { changelog }))
    }

    type PullStream = Pin<Box<dyn Stream<Item = Result<RegistryPullResponse, Status>> + Send>>;

    async fn pull(
        &self,
        request: Request<RegistryPullRequest>,
    ) -> Result<Response<Self::PullStream>, Status> {
        let request = request.into_inner();
        let mut args = registry_prefix(request.insecure, request.credentials.as_ref());
        args.extend([
            "pull".to_owned(),
            request.reference,
            "--destination".to_owned(),
            request.destination,
            "--validate".to_owned(),
        ]);
        let stream = pull_stream(args).await?;
        Ok(Response::new(Box::pin(stream)))
    }

    type ImageInstallStream =
        Pin<Box<dyn Stream<Item = Result<ImageInstallResponse, Status>> + Send>>;

    async fn image_install(
        &self,
        request: Request<ImageInstallRequest>,
    ) -> Result<Response<Self::ImageInstallStream>, Status> {
        validate_trust(&self.config)?;
        let request = request.into_inner();
        let mut args = vec![
            "image".to_owned(),
            "install".to_owned(),
            "--manifest".to_owned(),
            request.manifest.clone(),
            "--trusted-key".to_owned(),
            self.config.trusted_key.display().to_string(),
            "--uki-trusted-cert".to_owned(),
            self.config.uki_trusted_cert.display().to_string(),
            "--target".to_owned(),
            self.config.target.clone(),
            "--accepted-generation-file".to_owned(),
            self.config.accepted_generation_file.display().to_string(),
        ];
        let signature = if self.config.signature_path.as_os_str().is_empty() {
            format!("{}.sig", request.manifest)
        } else {
            self.config.signature_path.display().to_string()
        };
        args.extend(["--signature".to_owned(), signature]);
        let stream = output_stream::<ImageInstallResponse>(args).await?;
        Ok(Response::new(Box::pin(stream)))
    }

    type InstallCachixStream =
        Pin<Box<dyn Stream<Item = Result<SetGenerationResponse, Status>> + Send>>;

    async fn install_cachix(
        &self,
        request: Request<update::Cachix>,
    ) -> Result<Response<Self::InstallCachixStream>, Status> {
        let request = request.into_inner();
        let mut args = vec![
            "cachix".to_owned(),
            request.pin,
            "--cache".to_owned(),
            request.cache,
        ];
        if let Some(token) = request.token {
            args.extend(["--token".to_owned(), token]);
        }
        if let Some(host) = request.cachix_host {
            args.extend(["--cachix-host".to_owned(), host]);
        }
        let stream = output_stream::<SetGenerationResponse>(args).await?;
        Ok(Response::new(Box::pin(stream)))
    }
}

async fn run_command<I>(args: I) -> Result<std::process::Output, Status>
where
    I: IntoIterator<Item = String>,
{
    let output = Command::new("ota-update")
        .args(args)
        .output()
        .await
        .map_err(|err| Status::internal(format!("failed to execute ota-update: {err}")))?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(Status::unknown(format!(
            "ota-update failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

fn registry_prefix(insecure: bool, credentials: Option<&RegistryCredentials>) -> Vec<String> {
    let mut args = vec![
        "registry".to_owned(),
        "--output".to_owned(),
        "jsonl".to_owned(),
    ];
    if insecure {
        args.push("--insecure".to_owned());
    }
    if let Some(credentials) = credentials {
        match &credentials.auth {
            Some(update::registry_credentials::Auth::Basic(auth)) => args.extend([
                "--username".to_owned(),
                auth.username.clone(),
                "--password".to_owned(),
                auth.password.clone(),
            ]),
            Some(update::registry_credentials::Auth::Bearer(auth)) => {
                args.extend(["--token".to_owned(), auth.token.clone()]);
            }
            None => {}
        }
    }
    args
}

fn validate_trust(config: &UpdateConfig) -> Result<(), Status> {
    if config.trusted_key.as_os_str().is_empty()
        || config.uki_trusted_cert.as_os_str().is_empty()
        || config.target.is_empty()
    {
        return Err(Status::failed_precondition(
            "update trust policy is incomplete",
        ));
    }
    Ok(())
}

fn parse_jsonl_result<T: for<'de> Deserialize<'de>>(stdout: &[u8]) -> Result<T, Status> {
    if let Ok(result) = serde_json::from_slice(stdout) {
        return Ok(result);
    }

    let start = stdout
        .iter()
        .position(|byte| *byte == b'[')
        .ok_or_else(|| Status::internal("ota-update returned no JSON result"))?;
    serde_json::from_slice(&stdout[start..])
        .map_err(|err| Status::internal(format!("failed to parse ota-update result: {err}")))
}

#[derive(Debug, Deserialize)]
struct AvailableUpdateJson {
    repository: String,
    tag: String,
    version: String,
    hash: String,
}

impl From<AvailableUpdateJson> for AvailableUpdate {
    fn from(value: AvailableUpdateJson) -> Self {
        Self {
            repository: value.repository,
            tag: value.tag,
            version: value.version,
            hash: value.hash,
        }
    }
}

async fn pull_stream(
    args: Vec<String>,
) -> Result<ReceiverStream<Result<RegistryPullResponse, Status>>, Status> {
    let mut child = Command::new("ota-update")
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|err| Status::internal(format!("failed to execute ota-update: {err}")))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Status::internal("ota-update stdout unavailable"))?;
    let (tx, rx) = mpsc::channel(16);
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        let mut output_dir = None;
        let mut manifest_path = None;
        let mut saw_result = false;
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(path) = line.strip_prefix("pulled to: ") {
                output_dir = Some(path.to_owned());
            } else if let Some(path) = line.strip_prefix("manifest: ") {
                manifest_path = Some(path.to_owned());
            }
            if let Some(response) = parse_pull_line(&line) {
                saw_result = matches!(
                    &response.update,
                    Some(update::registry_pull_response::Update::Result(_))
                );
                if tx.send(Ok(response)).await.is_err() {
                    return;
                }
            }
        }
        match child.wait().await {
            Ok(status) if status.success() => {
                if !saw_result {
                    if let (Some(output_dir), Some(manifest_path)) = (output_dir, manifest_path) {
                        let _ = tx
                            .send(Ok(RegistryPullResponse {
                                update: Some(update::registry_pull_response::Update::Result(
                                    update::RegistryPullResult {
                                        output_dir,
                                        manifest_path,
                                    },
                                )),
                            }))
                            .await;
                    }
                }
            }
            Ok(status) => {
                let _ = tx
                    .send(Err(Status::unknown(format!(
                        "ota-update exited with {status}"
                    ))))
                    .await;
            }
            Err(err) => {
                let _ = tx
                    .send(Err(Status::internal(format!(
                        "failed waiting for ota-update: {err}"
                    ))))
                    .await;
            }
        }
    });
    Ok(ReceiverStream::new(rx))
}

fn parse_pull_line(line: &str) -> Option<RegistryPullResponse> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    if value.get("event").is_some() {
        let event: PullEvent = serde_json::from_value(value).ok()?;
        return Some(RegistryPullResponse {
            update: Some(update::registry_pull_response::Update::Progress(
                event.into(),
            )),
        });
    }
    let result: PullResult = serde_json::from_value(value).ok()?;
    Some(RegistryPullResponse {
        update: Some(update::registry_pull_response::Update::Result(
            update::RegistryPullResult {
                output_dir: result.output_dir,
                manifest_path: result.manifest_path,
            },
        )),
    })
}

async fn output_stream<T>(args: Vec<String>) -> Result<ReceiverStream<Result<T, Status>>, Status>
where
    T: FromOutputResponse + Send + 'static,
{
    let output = Command::new("ota-update")
        .args(args)
        .output()
        .await
        .map_err(|err| Status::internal(format!("failed to execute ota-update: {err}")))?;
    let finished = T::finished();
    let (tx, rx) = mpsc::channel(3);
    if output.status.success() {
        tx.send(Ok(T::output(
            String::from_utf8_lossy(&output.stdout).into_owned(),
        )))
        .await
        .map_err(|_| Status::internal("failed to send update output"))?;
    } else {
        tx.send(Ok(T::error(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )))
        .await
        .map_err(|_| Status::internal("failed to send update error"))?;
    }
    tx.send(Ok(finished))
        .await
        .map_err(|_| Status::internal("failed to send update completion"))?;
    Ok(ReceiverStream::new(rx))
}

trait FromOutputResponse: Sized {
    fn output(output: String) -> Self;
    fn error(error: String) -> Self;
    fn finished() -> Self;
}

impl FromOutputResponse for ImageInstallResponse {
    fn output(output: String) -> Self {
        Self {
            finished: false,
            output: Some(output),
            error: None,
        }
    }

    fn finished() -> Self {
        Self {
            finished: true,
            output: None,
            error: None,
        }
    }

    fn error(error: String) -> Self {
        Self {
            finished: false,
            output: None,
            error: Some(error),
        }
    }
}

impl FromOutputResponse for SetGenerationResponse {
    fn output(output: String) -> Self {
        Self {
            finished: false,
            output: Some(output),
            error: None,
        }
    }

    fn finished() -> Self {
        Self {
            finished: true,
            output: None,
            error: None,
        }
    }

    fn error(error: String) -> Self {
        Self {
            finished: false,
            output: None,
            error: Some(error),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum PullEvent {
    PullStarted {
        reference: String,
        destination: String,
    },
    BlobDownloading {
        digest: String,
        downloaded: u64,
        total: Option<u64>,
    },
    BlobVerified {
        digest: String,
    },
    ManifestWritten {
        path: String,
    },
    Cancelled {
        stage: String,
    },
    Done,
    #[serde(other)]
    Ignored,
}

impl From<PullEvent> for update::RegistryPullProgress {
    fn from(event: PullEvent) -> Self {
        use update::registry_pull_progress::Event;
        Self {
            event: Some(match event {
                PullEvent::PullStarted {
                    reference,
                    destination,
                } => Event::PullStarted(update::RegistryPullStarted {
                    reference,
                    destination,
                }),
                PullEvent::BlobDownloading {
                    digest,
                    downloaded,
                    total,
                } => Event::BlobDownloading(update::RegistryBlobDownloading {
                    digest,
                    downloaded,
                    total,
                }),
                PullEvent::BlobVerified { digest } => Event::BlobVerified(digest),
                PullEvent::ManifestWritten { path } => Event::ManifestWritten(path),
                PullEvent::Cancelled { stage } => Event::Cancelled(stage),
                PullEvent::Done => Event::Done(true),
                PullEvent::Ignored => return Self { event: None },
            }),
        }
    }
}

#[derive(Debug, Deserialize)]
struct PullResult {
    #[serde(rename = "outputDir")]
    output_dir: String,
    #[serde(rename = "manifestPath")]
    manifest_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GenerationDetails {
    generation: i32,
    nixos_version: String,
    kernel_version: String,
    configuration_revision: Option<String>,
    current: bool,
    store_path: String,
}

impl From<GenerationDetails> for Generation {
    fn from(value: GenerationDetails) -> Self {
        Self {
            generation: value.generation,
            date: String::new(),
            nixos_version: value.nixos_version,
            kernel_version: value.kernel_version,
            configuration_revision: value.configuration_revision.unwrap_or_default(),
            specialisations: Vec::new(),
            current: value.current,
            store_path: value.store_path,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_jsonl_result, registry_prefix, validate_trust};
    use crate::config::UpdateConfig;
    use givc_common::pb::update::{
        RegistryBasicAuth, RegistryCredentials, RegistryDiscoverRequest,
    };

    #[test]
    fn builds_registry_arguments_with_basic_auth() {
        let request = RegistryDiscoverRequest {
            reference: "registry.example/ghaf".to_owned(),
            insecure: true,
            credentials: Some(RegistryCredentials {
                auth: Some(givc_common::pb::update::registry_credentials::Auth::Basic(
                    RegistryBasicAuth {
                        username: "user".to_owned(),
                        password: "secret".to_owned(),
                    },
                )),
            }),
        };

        assert_eq!(
            registry_prefix(request.insecure, request.credentials.as_ref()),
            [
                "registry",
                "--output",
                "jsonl",
                "--insecure",
                "--username",
                "user",
                "--password",
                "secret"
            ]
        );
    }

    #[test]
    fn rejects_incomplete_image_trust_policy() {
        assert_eq!(
            validate_trust(&UpdateConfig::default())
                .expect_err("incomplete trust policy should fail")
                .code(),
            tonic::Code::FailedPrecondition
        );
    }

    #[test]
    fn parses_multiline_json_after_progress_events() {
        let output = br#"{"event":"done"}
[
  {"repository":"repo","tag":"tag","version":"1","hash":"sha256:x"}
]"#;
        let result: Vec<super::AvailableUpdateJson> =
            parse_jsonl_result(output).expect("result should parse");
        assert_eq!(result[0].tag, "tag");
    }
}
