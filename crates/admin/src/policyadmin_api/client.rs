// Copyright 2025 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use crate::pb::policyadmin::{
    StreamPolicyRequest, policy_admin_client::PolicyAdminClient as GrpcPolicyAdminClient,
};
use anyhow::Result;
use async_stream::stream;
use givc_client::endpoint::EndpointConfig;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio_stream::Stream;
use tonic::transport::Channel;
use tracing::{debug, error};

#[derive(Debug, Clone)]
pub struct PolicyAdminClient {
    endpoint: EndpointConfig,
}

impl PolicyAdminClient {
    pub fn new(endpoint: EndpointConfig) -> Self {
        Self { endpoint }
    }

    async fn connect(&self) -> Result<GrpcPolicyAdminClient<Channel>> {
        let client = self.endpoint.connect().await?;
        Ok(GrpcPolicyAdminClient::new(client))
    }

    pub async fn stream_policy(
        &self,
        updates: impl Stream<Item = StreamPolicyRequest> + Send + 'static,
    ) -> Result<()> {
        let mut client = self.connect().await?;
        let response = client.stream_policy(updates).await?.into_inner();
        if response.status != "Success" {
            return Err(anyhow::anyhow!("Policy update failed: {}", response.status));
        }
        debug!("stream_policy() successful");
        Ok(())
    }

    /**
     * Uploads a policy file to the target VM via a gRPC stream.
     *
     * This function initiates a client-side streaming RPC. It constructs a stream that
     * sends a sequence of `StreamPolicyRequest` messages:
     */

    pub async fn upload_policy(&self, name: String, path: String) -> Result<()> {
        debug!("Uploading policy: {}", name);
        let outbound_stream = stream! {
            let mut file = match File::open(&path).await {
                Ok(f) => f,
                Err(e) => {
                    error!("Failed to open policy file {}: {}", path, e);
                    return;
                }
            };

            // 3MB buffer
            let mut buffer = vec![0u8; 3 * 1024 * 1024];
            let mut is_first_chunk = true;

            loop {
                match file.read(&mut buffer).await {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        yield StreamPolicyRequest {
                            // Only send name and version on the first chunk to save bandwidth
                            policy_name: if is_first_chunk { name.clone() } else { String::new() },
                            policy_chunk: buffer[..n].to_vec(),
                        };
                        is_first_chunk = false;
                    }
                    Err(e) => {
                        error!("Error reading file chunk: {}", e);
                        break;
                    }
                }
            }
        };

        self.stream_policy(outbound_stream).await
    }
}
