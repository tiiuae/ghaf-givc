use anyhow::bail;
use tonic::Status;
use tracing::debug;

use crate::endpoint::EndpointConfig;
use crate::pb::SetGenerationResponse;
use crate::utils::tonic::{Stream, wrap_error};
use givc_client::exec::ExecClient;
use givc_common::pb::Generation;

type SetGenerationStream = Stream<SetGenerationResponse>;

#[allow(clippy::upper_case_acronyms)]
pub(crate) struct OTA {
    endpoint: EndpointConfig,
}

impl OTA {
    pub async fn connect(endpoint: EndpointConfig) -> anyhow::Result<Self> {
        Ok(Self { endpoint })
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
        let gens: Vec<Generation> = serde_json::from_slice(&stdout)?;
        Ok(gens)
    }

    // FIXME: Update going silently, it should report
    pub async fn set(
        &self,
        path: String,
        source: String,
        no_check_signs: bool,
    ) -> anyhow::Result<Stream<SetGenerationResponse>> {
        let mut exec = ExecClient::connect(self.endpoint.clone()).await?;
        let mut args = vec!["set".to_owned(), path, "--source".to_owned(), source];
        if no_check_signs {
            args.push("--no-check-signs".to_owned())
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
            if rc > 0 {
                return Err(wrap_error(anyhow::anyhow!(
                    "Execution of failed, RC code is {rc}"
                )));
            }
            emitter
                .emit(SetGenerationResponse {
                    finished: true,
                    output: None,
                    error: None,
                })
                .await;
            Ok(())
        });
        Ok(Box::pin(stream) as SetGenerationStream)
    }
}
