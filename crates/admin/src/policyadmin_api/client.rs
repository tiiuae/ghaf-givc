// Copyright 2025 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use crate::pb::policyadmin::{
    StreamPolicyRequest, policy_admin_client::PolicyAdminClient as GrpcPolicyAdminClient,
};
use anyhow::{Context, Result};
use async_stream::stream;
use givc_client::endpoint::EndpointConfig;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio_stream::Stream;
use tonic::transport::Channel;
use tracing::info;

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
        if response.status != "OK" {
            return Err(anyhow::anyhow!("Policy update failed: {}", response.status));
        }
        info!("stream_policy() successful");
        Ok(())
    }

    /**
     * Uploads a policy file to the target VM via a gRPC stream.
     *
     * This function initiates a client-side streaming RPC. It constructs a stream that
     * sends a sequence of `StreamPolicyRequest` messages:
     * 1. The first message contains the policy metadata (JSON) and an empty chunk.
     * 2. Subsequent messages contain chunks of the binary policy file (read from `file_path`)
     * with empty metadata.
     *
     * # Arguments
     *
     * * `metadata` - A JSON string containing policy metadata (e.g., version, ruleset ID).
     * * `file_path` - The filesystem path to the compiled policy file (e.g., `.wasm` or `.rego`).
     */

    pub async fn upload_policy(&self, metadata: String, file_path: String) -> Result<()> {
        /* Define the outbound stream that will yield requests to the gRPC server */
        let outbound_stream = stream! {
            /* Step 1: Send the initial packet containing only the metadata JSON */
            yield StreamPolicyRequest {
                metadata_json: metadata,
                policy_chunk: vec![],
            };

            /* Attempt to open the policy file asynchronously */
            let mut file = match File::open(&file_path).await {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("Failed to open policy file: {}", e);
                    return;
                }
            };

            /* Allocate a 3MB buffer for reading file chunks */
            let mut buffer = vec![0u8; 3 * 1024 * 1024];

            /* Step 2: Loop through the file, reading and yielding chunks */
            loop {
                match file.read(&mut buffer).await {
                    Ok(0) => break, /* End of file reached */
                    Ok(n) => {
                        /* Yield a chunk of the file. Metadata is empty for these packets */
                        yield StreamPolicyRequest {
                            metadata_json: String::new(),
                            policy_chunk: buffer[..n].to_vec(),
                        };
                    }
                    Err(e) => {
                        eprintln!("Error reading policy file: {}", e);
                        break;
                    }
                }
            }
        };

        /* Log the action and initiate the actual gRPC streaming call */
        info!("upload_policy() uploading policy...");
        self.stream_policy(outbound_stream).await
    }
}
