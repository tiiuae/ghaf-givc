// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::borrow::Cow;

use anyhow::{Context, ensure};
use tracing::debug;

use crate::endpoint::EndpointConfig;
use crate::pb::admin::{
    self, AvailableUpdate, ImageInstallRequest, ImageInstallResponse, RegistryPullProgress,
    RegistryPullResponse, RegistryPullResult, SetGenerationResponse,
};
use crate::utils::tonic::{AnyTonic, Stream};
use givc_client::exec::ExecClient;
use givc_common::pb::Generation;
use ota_update::registry::progress::RegistryEvent;
use ota_update::types::GenerationDetails;
use serde::Deserialize;

#[derive(Deserialize)]
struct DiscoverUpdate {
    repository: String,
    tag: String,
    version: String,
    hash: String,
}

pub(crate) type SetGenerationStream = Stream<SetGenerationResponse>;
pub(crate) type ImageInstallStream = Stream<ImageInstallResponse>;
pub(crate) type PullStream = Stream<RegistryPullResponse>;

struct RegistryArgs<'a> {
    action: Cow<'a, str>,
    reference: Cow<'a, str>,
    insecure: bool,
    credentials: Option<admin::RegistryCredentials>,
    destination: Option<Cow<'a, str>>,
    validate: bool,
}

impl<'a> RegistryArgs<'a> {
    fn new(action: impl Into<Cow<'a, str>>) -> Self {
        Self {
            action: action.into(),
            reference: "".into(),
            insecure: false,
            credentials: None,
            destination: None,
            validate: false,
        }
    }

    fn reference(self, reference: impl Into<Cow<'a, str>>) -> Self {
        Self {
            reference: reference.into(),
            ..self
        }
    }

    fn insecure(self, insecure: bool) -> Self {
        Self { insecure, ..self }
    }

    fn maybe_credentials(self, credentials: Option<admin::RegistryCredentials>) -> Self {
        Self {
            credentials,
            ..self
        }
    }

    fn destination(self, destination: impl Into<Cow<'a, str>>) -> Self {
        Self {
            destination: Some(destination.into()),
            ..self
        }
    }

    fn validate(self, validate: bool) -> Self {
        Self { validate, ..self }
    }

    fn into_args(self) -> Vec<String> {
        let mut args = vec![
            "registry".to_owned(),
            "--output".to_owned(),
            "jsonl".to_owned(),
        ];
        if self.insecure {
            args.push("--insecure".to_owned());
        }
        if let Some(credentials) = self.credentials {
            match credentials.auth {
                Some(admin::registry_credentials::Auth::Basic(basic)) => {
                    args.push("--username".to_owned());
                    args.push(basic.username);
                    args.push("--password".to_owned());
                    args.push(basic.password);
                }
                Some(admin::registry_credentials::Auth::Bearer(bearer)) => {
                    args.push("--token".to_owned());
                    args.push(bearer.token);
                }
                None => {}
            }
        }
        args.push(self.action.into_owned());
        args.push(self.reference.into_owned());
        if let Some(destination) = self.destination {
            args.push("--destination".to_owned());
            args.push(destination.into_owned());
        }
        if self.validate {
            args.push("--validate".to_owned());
        }
        args
    }
}

#[allow(clippy::upper_case_acronyms)]
pub(crate) struct OTA {
    endpoint: EndpointConfig,
}

impl OTA {
    pub(crate) fn new(endpoint: EndpointConfig) -> Self {
        Self { endpoint }
    }

    pub async fn list(&self) -> anyhow::Result<Vec<Generation>> {
        let mut exec = ExecClient::connect(self.endpoint.clone()).await?;
        let (stdout, stderr, rc) = exec
            .get_program_output(
                "ota-update".to_string(),
                vec!["get".to_string()],
                None,
                None,
                None,
                None,
            )
            .await?;
        ensure!(rc == 0, "Exec error: {}", String::from_utf8_lossy(&stderr));
        debug!("stdout: {}", String::from_utf8_lossy(&stdout));
        let gens: Vec<GenerationDetails> = serde_json::from_slice(&stdout)?;
        gens.into_iter()
            .map(|g| {
                Ok(Generation {
                    current: g.current,
                    generation: g.generation,
                    store_path: g
                        .store_path
                        .into_os_string()
                        .into_string()
                        .ok()
                        .context("Decode UTF-8")?,
                    configuration_revision: g
                        .configuration_revision
                        .unwrap_or_else(|| "unknown".into()),
                    nixos_version: g.nixos_version,
                    kernel_version: g.kernel_version,
                    specialisations: Vec::new(),
                    date: "bogus".into(),
                })
            })
            .collect()
    }

    pub async fn discover(
        &self,
        request: admin::RegistryDiscoverRequest,
    ) -> anyhow::Result<Vec<AvailableUpdate>> {
        let mut exec = ExecClient::connect(self.endpoint.clone()).await?;
        let args = RegistryArgs::new("discover")
            .reference(request.reference)
            .insecure(request.insecure)
            .maybe_credentials(request.credentials)
            .into_args();
        let (stdout, stderr, rc) = exec
            .get_program_output("ota-update".to_string(), args, None, None, None, None)
            .await?;
        ensure!(rc == 0, "Exec error: {}", String::from_utf8_lossy(&stderr));
        let output = Self::after_done_output(&stdout)?;
        debug!("discover stdout: {output}");
        let updates: Vec<DiscoverUpdate> = serde_json::from_str(&output)?;
        Ok(updates
            .into_iter()
            .map(|item| AvailableUpdate {
                repository: item.repository,
                tag: item.tag,
                version: item.version,
                hash: item.hash,
            })
            .collect())
    }

    pub async fn changelog(
        &self,
        request: admin::RegistryChangelogRequest,
    ) -> anyhow::Result<String> {
        let mut exec = ExecClient::connect(self.endpoint.clone()).await?;
        let args = RegistryArgs::new("changelog")
            .reference(request.reference)
            .insecure(request.insecure)
            .maybe_credentials(request.credentials)
            .into_args();
        let (stdout, stderr, rc) = exec
            .get_program_output("ota-update".to_string(), args, None, None, None, None)
            .await?;
        ensure!(rc == 0, "Exec error: {}", String::from_utf8_lossy(&stderr));
        let output = Self::after_done_output(&stdout)?;
        debug!("changelog stdout: {output}");
        Ok(output)
    }

    pub async fn pull(&self, request: admin::RegistryPullRequest) -> anyhow::Result<PullStream> {
        let mut exec = ExecClient::connect(self.endpoint.clone()).await?;
        let args = RegistryArgs::new("pull")
            .reference(request.reference)
            .insecure(request.insecure)
            .maybe_credentials(request.credentials)
            .destination(request.destination)
            .validate(request.validate)
            .into_args();
        let stream = async_fn_stream::try_fn_stream(async move |emitter| {
            debug!("Invoke ota-update: {args:?}");
            let mut stdout_buf = Vec::new();
            let mut result_buf = String::new();
            let mut seen_done = false;
            let rc = exec
                .start_command(
                    "ota-update".to_string(),
                    args,
                    None,
                    None,
                    None,
                    None,
                    |stdout| {
                        stdout_buf.extend(stdout);
                        let emitter = &emitter;
                        let events = Self::drain_stdout_lines(
                            &mut stdout_buf,
                            &mut seen_done,
                            &mut result_buf,
                        );
                        async move {
                            for event in events {
                                if let Some(progress) = Self::registry_event_to_progress(event) {
                                    emitter
                                        .emit(RegistryPullResponse {
                                            update: Some(
                                                admin::registry_pull_response::Update::Progress(
                                                    progress,
                                                ),
                                            ),
                                        })
                                        .await;
                                }
                            }
                        }
                    },
                    |stderr| {
                        let err = String::from_utf8_lossy(&stderr);
                        debug!("stderr: {}", err);
                        std::future::ready(())
                    },
                )
                .await
                .status("Executing ota-update failed")?;
            let events = Self::flush_stdout(&mut stdout_buf, &mut seen_done, &mut result_buf);
            for event in events {
                if let Some(progress) = Self::registry_event_to_progress(event) {
                    emitter
                        .emit(RegistryPullResponse {
                            update: Some(admin::registry_pull_response::Update::Progress(progress)),
                        })
                        .await;
                }
            }
            (rc == 0).with_status(|| format!("Execution of ota-update failed, RC code is {rc}"))?;

            let result =
                Self::parse_pull_result(&result_buf).status("Failed to parse pull result")?;
            emitter
                .emit(RegistryPullResponse {
                    update: Some(admin::registry_pull_response::Update::Result(result)),
                })
                .await;
            Ok(())
        });
        Ok(Box::pin(stream) as PullStream)
    }

    pub async fn image_install(
        &self,
        request: ImageInstallRequest,
    ) -> anyhow::Result<ImageInstallStream> {
        let mut exec = ExecClient::connect(self.endpoint.clone()).await?;
        let mut args = vec![
            "image".to_owned(),
            "install".to_owned(),
            "--manifest".to_owned(),
            request.manifest,
        ];
        if request.validate {
            args.push("--validate".to_owned());
        }
        let stream = async_fn_stream::try_fn_stream(async move |emitter| {
            debug!("Invoke ota-update: {args:?}");
            let rc = exec
                .start_command(
                    "ota-update".to_string(),
                    args,
                    None,
                    None,
                    None,
                    None,
                    |stdout| {
                        let out = String::from_utf8_lossy(&stdout);
                        debug!("stdout: {}", out);
                        emitter.emit(ImageInstallResponse {
                            finished: false,
                            output: Some(out.into()),
                            error: None,
                        })
                    },
                    |stderr| {
                        let err = String::from_utf8_lossy(&stderr);
                        debug!("stderr: {}", err);
                        emitter.emit(ImageInstallResponse {
                            finished: false,
                            output: None,
                            error: Some(err.into()),
                        })
                    },
                )
                .await
                .status("Failed to invoke ota-update")?;
            let () = emitter
                .emit(ImageInstallResponse {
                    finished: true,
                    output: None,
                    error: None,
                })
                .await;
            (rc == 0).with_status(|| format!("Execution of ota-update failed, RC code is {rc}"))?;
            Ok(())
        });
        Ok(Box::pin(stream) as ImageInstallStream)
    }

    // FIXME: Update going silently, it should report
    pub async fn install_via_cachix(
        &self,
        cachix_request: crate::pb::admin::Cachix,
    ) -> anyhow::Result<Stream<SetGenerationResponse>> {
        let mut exec = ExecClient::connect(self.endpoint.clone()).await?;
        let mut args = vec![
            "cachix".to_owned(),
            cachix_request.pin,
            "--cache".to_owned(),
            cachix_request.cache,
        ];
        if let Some(token) = cachix_request.token {
            args.push("--token".to_owned());
            args.push(token);
        }
        if let Some(cachix_host) = cachix_request.cachix_host {
            args.push("--cachix-host".to_owned());
            args.push(cachix_host);
        }
        let stream = async_fn_stream::try_fn_stream(async move |emitter| {
            debug!("Invoke ota-update: {args:?}");
            let rc = exec
                .start_command(
                    "ota-update".to_string(),
                    args,
                    None,
                    None,
                    None,
                    None,
                    |stdout| {
                        let out = String::from_utf8_lossy(&stdout);
                        debug!("stdout: {}", out);
                        emitter.emit(SetGenerationResponse {
                            finished: false,
                            output: Some(out.into()),
                            error: None,
                        })
                    },
                    |stderr| {
                        let err = String::from_utf8_lossy(&stderr);
                        debug!("stderr: {}", err);
                        emitter.emit(SetGenerationResponse {
                            finished: false,
                            output: None,
                            error: Some(err.into()),
                        })
                    },
                )
                .await
                .status("Failed to invoke ota-update")?;
            emitter
                .emit(SetGenerationResponse {
                    finished: true,
                    output: None,
                    error: None,
                })
                .await;
            (rc == 0).with_status(|| format!("Execution of ota-update failed, RC code is {rc}"))?;
            Ok(())
        });
        Ok(Box::pin(stream) as SetGenerationStream)
    }

    fn drain_stdout_lines(
        stdout_buf: &mut Vec<u8>,
        seen_done: &mut bool,
        result_buf: &mut String,
    ) -> Vec<RegistryEvent> {
        let mut events = Vec::new();
        while let Some(line) = Self::next_stdout_line(stdout_buf) {
            if let Some(event) = Self::handle_stdout_line(&line, seen_done, result_buf) {
                events.push(event);
            }
        }
        events
    }

    fn flush_stdout(
        stdout_buf: &mut Vec<u8>,
        seen_done: &mut bool,
        result_buf: &mut String,
    ) -> Vec<RegistryEvent> {
        let mut events = Vec::new();
        if !stdout_buf.is_empty() {
            let line = std::mem::take(stdout_buf);
            if let Some(event) = Self::handle_stdout_line(&line, seen_done, result_buf) {
                events.push(event);
            }
        }
        events
    }

    fn handle_stdout_line(
        line: &[u8],
        seen_done: &mut bool,
        result_buf: &mut String,
    ) -> Option<RegistryEvent> {
        if line.is_empty() {
            return None;
        }

        if let Ok(event) = serde_json::from_slice::<RegistryEvent>(line) {
            if matches!(event, RegistryEvent::Done) {
                *seen_done = true;
            }
            return Some(event);
        }

        if *seen_done {
            if !result_buf.is_empty() {
                result_buf.push('\n');
            }
            result_buf.push_str(&String::from_utf8_lossy(line));
        } else {
            debug!("registry stdout: {}", String::from_utf8_lossy(line));
        }
        None
    }

    fn next_stdout_line(stdout_buf: &mut Vec<u8>) -> Option<Vec<u8>> {
        let newline_pos = stdout_buf.iter().position(|byte| *byte == b'\n')?;
        let mut line = stdout_buf.drain(..=newline_pos).collect::<Vec<_>>();
        if line.last() == Some(&b'\n') {
            line.pop();
        }
        Some(line)
    }

    fn after_done_output(stdout: &[u8]) -> anyhow::Result<String> {
        let mut seen_done = false;
        let mut output = String::new();
        for raw_line in stdout.split(|byte| *byte == b'\n') {
            if raw_line.is_empty() {
                continue;
            }
            if let Ok(event) = serde_json::from_slice::<RegistryEvent>(raw_line) {
                if matches!(event, RegistryEvent::Done) {
                    seen_done = true;
                }
                continue;
            }
            if seen_done {
                if !output.is_empty() {
                    output.push('\n');
                }
                output.push_str(std::str::from_utf8(raw_line)?);
            }
        }
        Ok(output)
    }

    fn parse_pull_result(stdout: &str) -> anyhow::Result<RegistryPullResult> {
        let mut output_dir = None;
        let mut manifest_path = None;

        for line in stdout.lines() {
            if let Some(value) = line.strip_prefix("pulled to: ") {
                output_dir = Some(value.to_owned());
            } else if let Some(value) = line.strip_prefix("manifest: ") {
                manifest_path = Some(value.to_owned());
            }
        }

        Ok(RegistryPullResult {
            output_dir: output_dir.context("pull output_dir missing from command output")?,
            manifest_path: manifest_path
                .context("pull manifest_path missing from command output")?,
        })
    }

    fn registry_event_to_progress(event: RegistryEvent) -> Option<RegistryPullProgress> {
        use admin::registry_pull_progress::Event;

        let event = match event {
            RegistryEvent::PullStarted {
                reference,
                destination,
            } => Event::PullStarted(admin::RegistryPullStarted {
                reference,
                destination,
            }),
            RegistryEvent::BlobDownloading {
                digest,
                downloaded,
                total,
            } => Event::BlobDownloading(admin::RegistryBlobDownloading {
                digest,
                downloaded,
                total,
            }),
            RegistryEvent::BlobVerified { digest } => Event::BlobVerified(digest),
            RegistryEvent::ManifestWritten { path } => {
                Event::ManifestWritten(path.display().to_string())
            }
            RegistryEvent::Cancelled { stage } => Event::Cancelled(stage),
            RegistryEvent::Done => Event::Done(true),
            _ => return None,
        };
        Some(RegistryPullProgress { event: Some(event) })
    }
}
