// Copyright 2025 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use crate::pb::policyagent::{
    StreamPolicyRequest, policy_agent_client::PolicyAgentClient as GrpcPolicyAgentClient,
};
use anyhow::Result;
use futures_util::stream::Stream;
use givc_client::endpoint::EndpointConfig;
use tonic::transport::Channel;

#[derive(Debug, Clone)]
pub struct PolicyAgentClient {
    endpoint: EndpointConfig,
}

impl PolicyAgentClient {
    pub fn new(endpoint: EndpointConfig) -> Self {
        Self { endpoint }
    }

    async fn connect(&self) -> Result<GrpcPolicyAgentClient<Channel>> {
        let client = self.endpoint.connect().await?;
        Ok(GrpcPolicyAgentClient::new(client))
    }

    pub async fn stream_policy(
        &self,
        updates: impl Stream<Item = StreamPolicyRequest> + Send + 'static,
    ) -> Result<()> {
        let mut client = self.connect().await?;
        let response = client.stream_policy(updates).await?.into_inner();
        if response.status != "OK" {
            return Err(anyhow::anyhow!("Policy update failed: {}", response.status));
        }
        Ok(())
    }
}
