use crate::pb::{self, *};
use crate::types::*;
use anyhow::Result;
use givc_client::endpoint::EndpointConfig;
use tonic::transport::Channel;
use tonic::{metadata::MetadataValue, Code, Request, Response, Status};

type Client = pb::systemd::unit_control_service_client::UnitControlServiceClient<Channel>;

#[derive(Debug)]
pub struct SystemDClient {
    endpoint: EndpointConfig,
}

impl SystemDClient {
    pub fn new(ec: EndpointConfig) -> Self {
        Self { endpoint: ec }
    }

    async fn connect(&self) -> anyhow::Result<Client> {
        let channel = self.endpoint.connect().await?;
        Ok(Client::new(channel))
    }

    pub async fn get_remote_status(
        &self,
        unit: String,
    ) -> anyhow::Result<crate::types::UnitStatus> {
        let request = pb::systemd::UnitRequest { unit_name: unit };
        let response = self.connect().await?.get_unit_status(request).await?;
        let status = response
            .into_inner()
            .unit_status
            .ok_or_else(|| anyhow::anyhow!("missing unit_status field"))?;
        Ok(crate::types::UnitStatus {
            name: status.name,
            description: status.description,
            load_state: status.load_state,
            active_state: status.active_state,
            sub_state: status.sub_state,
            path: status.path,
        })
    }

    pub async fn start_remote(&self, unit: String) -> anyhow::Result<String> {
        let request = pb::systemd::UnitRequest { unit_name: unit };
        let resp = self.connect().await?.start_unit(request).await?;
        let status = resp.into_inner();
        Ok(status.cmd_status)
    }

    pub async fn stop_remote(&self, unit: String) -> anyhow::Result<String> {
        let request = pb::systemd::UnitRequest { unit_name: unit };
        let resp = self.connect().await?.stop_unit(request).await?;
        let status = resp.into_inner();
        Ok(status.cmd_status)
    }

    pub async fn kill_remote(&self, unit: String) -> anyhow::Result<String> {
        let request = pb::systemd::UnitRequest { unit_name: unit };
        let resp = self.connect().await?.kill_unit(request).await?;
        let status = resp.into_inner();
        Ok(status.cmd_status)
    }

    pub async fn pause_remote(&self, unit: String) -> anyhow::Result<String> {
        let request = pb::systemd::UnitRequest { unit_name: unit };
        let resp = self.connect().await?.freeze_unit(request).await?;
        let status = resp.into_inner();
        Ok(status.cmd_status)
    }

    pub async fn resume_remote(&self, unit: String) -> anyhow::Result<String> {
        let request = pb::systemd::UnitRequest { unit_name: unit };
        let resp = self.connect().await?.unfreeze_unit(request).await?;
        let status = resp.into_inner();
        Ok(status.cmd_status)
    }

    pub async fn start_application(&self, unit: String) -> anyhow::Result<String> {
        let request = pb::systemd::UnitRequest { unit_name: unit };
        let resp = self.connect().await?.start_application(request).await?;
        let status = resp.into_inner();
        Ok(status.cmd_status)
    }
}
