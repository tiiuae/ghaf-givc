use crate::pb;
use givc_client::endpoint::EndpointConfig;
use givc_client::error::StatusWrapExt;
use pb::systemd::UnitResponse;
use tonic::transport::Channel;
use tracing::debug;

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

    fn status_response(
        &self,
        response: tonic::Response<UnitResponse>,
    ) -> anyhow::Result<crate::types::UnitStatus> {
        let status = response
            .into_inner()
            .unit_status
            .ok_or_else(|| anyhow::anyhow!("missing unit_status field"))?;
        let us = crate::types::UnitStatus {
            name: status.name,
            description: status.description,
            load_state: status.load_state,
            active_state: status.active_state,
            sub_state: status.sub_state,
            path: status.path,
            freezer_state: status.freezer_state,
        };
        debug!("Got remote status: {:?}", us);
        Ok(us)
    }

    pub async fn get_remote_status(
        &self,
        unit: String,
    ) -> anyhow::Result<crate::types::UnitStatus> {
        let request = pb::systemd::UnitRequest { unit_name: unit };
        let response = self
            .connect()
            .await?
            .get_unit_status(request)
            .await
            .rewrap_err()?;
        self.status_response(response)
    }

    pub async fn start_remote(&self, unit: String) -> anyhow::Result<crate::types::UnitStatus> {
        let request = pb::systemd::UnitRequest { unit_name: unit };
        let response = self
            .connect()
            .await?
            .start_unit(request)
            .await
            .rewrap_err()?;
        self.status_response(response)
    }

    pub async fn stop_remote(&self, unit: String) -> anyhow::Result<crate::types::UnitStatus> {
        let request = pb::systemd::UnitRequest { unit_name: unit };
        let response = self
            .connect()
            .await?
            .stop_unit(request)
            .await
            .rewrap_err()?;
        self.status_response(response)
    }

    pub async fn kill_remote(&self, unit: String) -> anyhow::Result<crate::types::UnitStatus> {
        let request = pb::systemd::UnitRequest { unit_name: unit };
        let response = self
            .connect()
            .await?
            .kill_unit(request)
            .await
            .rewrap_err()?;
        self.status_response(response)
    }

    pub async fn pause_remote(&self, unit: String) -> anyhow::Result<crate::types::UnitStatus> {
        let request = pb::systemd::UnitRequest { unit_name: unit };
        let response = self
            .connect()
            .await?
            .freeze_unit(request)
            .await
            .rewrap_err()?;
        self.status_response(response)
    }

    pub async fn resume_remote(&self, unit: String) -> anyhow::Result<crate::types::UnitStatus> {
        let request = pb::systemd::UnitRequest { unit_name: unit };
        let response = self
            .connect()
            .await?
            .unfreeze_unit(request)
            .await
            .rewrap_err()?;
        self.status_response(response)
    }

    pub async fn start_application(
        &self,
        unit: String,
        args: Vec<String>,
    ) -> anyhow::Result<crate::types::UnitStatus> {
        let request = pb::systemd::AppUnitRequest {
            unit_name: unit,
            args,
        };
        let response = self
            .connect()
            .await?
            .start_application(request)
            .await
            .rewrap_err()?;
        self.status_response(response)
    }
}
