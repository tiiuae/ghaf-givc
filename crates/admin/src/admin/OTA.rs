// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, bail};
use tonic::Status;
use tracing::debug;

use crate::endpoint::EndpointConfig;
use crate::pb::SetGenerationResponse;
use crate::utils::tonic::{Stream, wrap_error};
use givc_client::exec::ExecClient;
use givc_common::pb::Generation;
use ota_update::types::GenerationDetails;

type SetGenerationStream = Stream<SetGenerationResponse>;

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
        if rc > 0 {
            bail!("Exec error: {}", String::from_utf8_lossy(&stderr))
        }
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
                .map_err(|e| Status::unknown(e.to_string()))?;
            emitter
                .emit(SetGenerationResponse {
                    finished: true,
                    output: None,
                    error: None,
                })
                .await;
            if rc > 0 {
                return Err(wrap_error(anyhow::anyhow!(
                    "Execution of ota-update failed, RC code is {rc}"
                )));
            }
            Ok(())
        });
        Ok(Box::pin(stream) as SetGenerationStream)
    }
}
