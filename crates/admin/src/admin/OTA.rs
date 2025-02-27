use anyhow::bail;
use tracing::info;

use crate::endpoint::EndpointConfig;
use givc_client::exec::ExecClient;
use givc_common::pb::Generation;

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
            .start_command(
                "ota-update".to_string(),
                vec!["--get".to_string()],
                None,
                None,
                None,
                None,
            )
            .await?;
        if rc > 0 {
            bail!("Exec error: {}", String::from_utf8_lossy(&stderr))
        }
        info!("stdout: {}", String::from_utf8_lossy(&stdout));
        let gens: Vec<Generation> = serde_json::from_slice(&stdout)?;
        Ok(gens)
    }

    // FIXME: Update going silently, it should report
    pub async fn set(&self, path: String) -> anyhow::Result<()> {
        let mut exec = ExecClient::connect(self.endpoint.clone()).await?;
        let args = vec!["--set".to_owned(), path];
        let (stdout, stderr, rc) = exec
            .start_command("ota-updater".to_string(), args, None, None, None, None)
            .await?;
        info!("stderr: {}", String::from_utf8_lossy(&stderr));
        info!("stdout: {}", String::from_utf8_lossy(&stdout));
        Ok(())
    }
}
