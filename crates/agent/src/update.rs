use std::ffi::OsStr;
use std::pin::Pin;

use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
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
        args.extend(["discover", &request.reference]);
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
        args.extend(["changelog", &request.reference]);
        let output = run_command(args).await?;
        let changelog = changelog_output(&output.stdout);
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
            "pull",
            &request.reference,
            "--destination",
            &request.destination,
            "--validate",
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

async fn run_command<I, S>(args: I) -> Result<std::process::Output, Status>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
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

fn registry_prefix(insecure: bool, credentials: Option<&RegistryCredentials>) -> Vec<&str> {
    let mut args = vec!["registry", "--output", "jsonl"];
    if insecure {
        args.push("--insecure");
    }
    if let Some(credentials) = credentials {
        match &credentials.auth {
            Some(update::registry_credentials::Auth::Basic(auth)) => {
                args.extend(["--username", &auth.username, "--password", &auth.password])
            }
            Some(update::registry_credentials::Auth::Bearer(auth)) => {
                args.extend(["--token", &auth.token]);
            }
            None => {}
        }
    }
    args
}

fn validate_trust(config: &UpdateConfig) -> Result<(), Status> {
    if config.trusted_key.is_empty()
        || config.uki_trusted_cert.is_empty()
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

async fn pull_stream<S>(
    args: Vec<S>,
) -> Result<ReceiverStream<Result<RegistryPullResponse, Status>>, Status>
where
    S: AsRef<OsStr>,
{
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
        let mut has_result = false;
        let mut stream_error = None;
        loop {
            let line = match lines.next_line().await {
                Ok(Some(line)) => line,
                Ok(None) => break,
                Err(err) => {
                    stream_error = Some(Status::internal(format!(
                        "failed reading ota-update output: {err}"
                    )));
                    let _ = child.kill().await;
                    break;
                }
            };

            let response = match parse_pull_line(&line) {
                Ok(Some(response)) => response,
                Ok(None) => continue,
                Err(err) => {
                    stream_error = Some(err);
                    let _ = child.kill().await;
                    break;
                }
            };
            has_result = matches!(
                &response.update,
                Some(update::registry_pull_response::Update::Result(_))
            );
            if tx.send(Ok(response)).await.is_err() {
                return;
            }
        }

        let wait_result = child.wait().await;
        if let Some(err) = stream_error {
            let _ = tx.send(Err(err)).await;
            return;
        }

        match wait_result {
            Ok(status) if status.success() => {
                if !has_result {
                    let _ = tx
                        .send(Err(Status::internal("ota-update returned no pull result")))
                        .await;
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

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum PullOutput {
    Event(PullEvent),
    Result(PullResult),
}

fn parse_pull_line(line: &str) -> Result<Option<RegistryPullResponse>, Status> {
    let output: PullOutput = serde_json::from_str(line)
        .map_err(|err| Status::internal(format!("failed to parse ota-update JSONL: {err}")))?;

    match output {
        PullOutput::Event(PullEvent::Ignored) => Ok(None),
        PullOutput::Event(event) => Ok(Some(RegistryPullResponse {
            update: Some(update::registry_pull_response::Update::Progress(
                event.into(),
            )),
        })),
        PullOutput::Result(result) => Ok(Some(RegistryPullResponse {
            update: Some(update::registry_pull_response::Update::Result(
                update::RegistryPullResult {
                    output_dir: result.output_dir,
                    manifest_path: result.manifest_path,
                },
            )),
        })),
    }
}

async fn output_stream<T>(args: Vec<String>) -> Result<ReceiverStream<Result<T, Status>>, Status>
where
    T: FromOutputResponse + Send + 'static,
{
    output_stream_from("ota-update", args).await
}

async fn output_stream_from<T>(
    program: &str,
    args: Vec<String>,
) -> Result<ReceiverStream<Result<T, Status>>, Status>
where
    T: FromOutputResponse + Send + 'static,
{
    let mut child = Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|err| Status::internal(format!("failed to execute ota-update: {err}")))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Status::internal("ota-update stdout unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| Status::internal("ota-update stderr unavailable"))?;
    let (tx, rx) = mpsc::channel(3);
    tokio::spawn(async move {
        let stdout_tx = tx.clone();
        let stdout_task = tokio::spawn(async move {
            let mut stdout = stdout;
            let mut buffer = [0_u8; 1024];
            loop {
                match stdout.read(&mut buffer).await {
                    Ok(0) => return Ok::<(), Status>(()),
                    Ok(count) => {
                        if stdout_tx
                            .send(Ok(T::output(
                                String::from_utf8_lossy(&buffer[..count]).into_owned(),
                            )))
                            .await
                            .is_err()
                        {
                            return Ok(());
                        }
                    }
                    Err(err) => {
                        return Err(Status::internal(format!(
                            "failed reading update output: {err}"
                        )));
                    }
                }
            }
        });
        let stderr_tx = tx.clone();
        let stderr_task = tokio::spawn(async move {
            let mut stderr = stderr;
            let mut buffer = [0_u8; 1024];
            loop {
                match stderr.read(&mut buffer).await {
                    Ok(0) => return Ok::<(), Status>(()),
                    Ok(count) => {
                        if stderr_tx
                            .send(Ok(T::error(
                                String::from_utf8_lossy(&buffer[..count]).into_owned(),
                            )))
                            .await
                            .is_err()
                        {
                            return Ok(());
                        }
                    }
                    Err(err) => {
                        return Err(Status::internal(format!(
                            "failed reading update error: {err}"
                        )));
                    }
                }
            }
        });
        let status = child.wait().await;
        let stdout_result = stdout_task.await;
        let stderr_result = stderr_task.await;
        if let Ok(Err(err)) = stdout_result {
            let _ = tx.send(Err(err)).await;
            return;
        }
        if let Ok(Err(err)) = stderr_result {
            let _ = tx.send(Err(err)).await;
            return;
        }
        match status {
            Ok(status) if status.success() => {
                let _ = tx.send(Ok(T::finished())).await;
            }
            Ok(status) => {
                let _ = tx
                    .send(Err(Status::unknown(format!(
                        "ota-update failed with rc {}",
                        status.code().unwrap_or(-1)
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
    #[serde(alias = "outputDir")]
    output_dir: String,
    #[serde(alias = "manifestPath")]
    manifest_path: String,
}

fn changelog_output(stdout: &[u8]) -> String {
    let mut after_done = false;
    let mut output = Vec::new();
    for line in stdout.split(|byte| *byte == b'\n') {
        if let Ok(value) = serde_json::from_slice::<serde_json::Value>(line) {
            if let Some(event) = value.get("event").and_then(serde_json::Value::as_str) {
                if event == "done" {
                    after_done = true;
                }
                continue;
            }
        }
        if after_done {
            output.push(String::from_utf8_lossy(line).into_owned());
        }
    }
    output.join("\n").trim().to_owned()
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
    use super::{parse_jsonl_result, parse_pull_line, registry_prefix, validate_trust};
    use crate::config::UpdateConfig;
    use givc_common::pb::update::{
        self, RegistryBasicAuth, RegistryCredentials, RegistryDiscoverRequest,
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

    #[test]
    fn parses_pull_progress_jsonl() {
        let response = parse_pull_line(r#"{"event":"blob_verified","digest":"sha256:test"}"#)
            .expect("progress line should parse")
            .expect("known event should produce a response");

        assert!(matches!(
            response.update,
            Some(update::registry_pull_response::Update::Progress(_))
        ));
    }

    #[test]
    fn parses_pull_result_jsonl() {
        let response = parse_pull_line(
            r#"{"outputDir":"/persist/sysupdate/update","manifestPath":"/persist/sysupdate/update/manifest.json"}"#,
        )
        .expect("result line should parse")
        .expect("result should produce a response");

        let Some(update::registry_pull_response::Update::Result(result)) = response.update else {
            panic!("expected pull result");
        };
        assert_eq!(result.output_dir, "/persist/sysupdate/update");
        assert_eq!(
            result.manifest_path,
            "/persist/sysupdate/update/manifest.json"
        );
    }

    #[test]
    fn parses_cli_snake_case_pull_result_jsonl() {
        let response = parse_pull_line(
            r#"{"output_dir":"/persist/update","manifest_path":"/persist/update/manifest.json"}"#,
        )
        .expect("CLI result should parse")
        .expect("result should produce a response");

        let Some(update::registry_pull_response::Update::Result(result)) = response.update else {
            panic!("expected pull result");
        };
        assert_eq!(result.output_dir, "/persist/update");
        assert_eq!(result.manifest_path, "/persist/update/manifest.json");
    }

    #[test]
    fn strips_discover_progress_and_preserves_changelog_after_done() {
        let discover = br#"{"event":"done"}
[{"repository":"one","tag":"v1","version":"1","hash":"a"},{"repository":"two","tag":"v2","version":"2","hash":"b"}]"#;
        let updates: Vec<super::AvailableUpdateJson> = parse_jsonl_result(discover).unwrap();
        assert_eq!(
            updates.iter().map(|u| u.tag.as_str()).collect::<Vec<_>>(),
            ["v1", "v2"]
        );

        let changelog = super::changelog_output(b"prelude\n{\"event\":\"done\"}\nfirst\nsecond\n");
        assert_eq!(changelog, "first\nsecond");
    }

    #[tokio::test]
    async fn cachix_streams_stdout_then_reports_nonzero_exit() {
        use super::output_stream_from;
        use tokio_stream::StreamExt;

        let output = output_stream_from::<givc_common::pb::update::SetGenerationResponse>(
            "/bin/sh",
            vec!["-c".to_owned(), "printf 'cache output'; exit 0".to_owned()],
        )
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;
        assert_eq!(
            output[0].as_ref().unwrap().output.as_deref(),
            Some("cache output")
        );
        assert!(output[1].as_ref().unwrap().finished);

        let failure = output_stream_from::<givc_common::pb::update::SetGenerationResponse>(
            "/bin/sh",
            vec!["-c".to_owned(), "printf failure >&2; exit 7".to_owned()],
        )
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;
        assert_eq!(
            failure[0].as_ref().unwrap().error.as_deref(),
            Some("failure")
        );
        assert_eq!(
            failure.last().unwrap().as_ref().unwrap_err().code(),
            tonic::Code::Unknown
        );
    }

    #[tokio::test]
    async fn output_stream_returns_before_subprocess_finishes() {
        let stream = tokio::time::timeout(
            std::time::Duration::from_millis(50),
            super::output_stream_from::<givc_common::pb::update::SetGenerationResponse>(
                "/bin/sh",
                vec![
                    "-c".to_owned(),
                    "printf first; sleep 0.2; printf second".to_owned(),
                ],
            ),
        )
        .await
        .expect("stream setup must not wait for process completion")
        .unwrap();
        drop(stream);
    }

    #[test]
    fn rejects_invalid_pull_jsonl() {
        assert!(parse_pull_line("not json").is_err());
    }
}
