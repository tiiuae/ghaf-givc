// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use tonic::transport::Channel;

use crate::endpoint::EndpointConfig;
use crate::error::StatusWrapExt;
use givc_common::pb;
use givc_common::pb::Generation;

type Client = givc_common::pb::update::update_client::UpdateClient<Channel>;

/// `UpdateClient` struct for interacting with the update gRPC server
pub struct UpdateClient {
    client: Client,
}

impl UpdateClient {
    /// Connects to the gRPC server at the specified address
    /// # Errors
    /// Raise error if unable to connect
    pub async fn connect(endpoint: EndpointConfig) -> anyhow::Result<Self> {
        let channel = endpoint.connect().await?;
        let client = Client::new(channel);
        Ok(Self { client })
    }

    /// List installed generations (updates)
    /// # Errors
    /// Fails if remote execution of `ota-update` tool failed, or on network IO errors
    pub async fn list_generations(&mut self) -> anyhow::Result<Vec<Generation>> {
        let response = self
            .client
            .list_generations(pb::admin::Empty {})
            .await
            .rewrap_err()?;
        Ok(response.into_inner().list)
    }

    /// Discover OTA updates in a registry repository.
    /// # Errors
    /// Fails if remote execution of `ota-update` tool failed, or on network IO errors
    pub async fn discover_updates(
        &mut self,
        reference: String,
        auth: Option<crate::RegistryAuth>,
        insecure: bool,
    ) -> anyhow::Result<Vec<pb::admin::AvailableUpdate>> {
        let request = pb::admin::RegistryDiscoverRequest {
            reference,
            insecure,
            credentials: Self::registry_credentials(auth),
        };
        Ok(self
            .client
            .discover(request)
            .await
            .rewrap_err()?
            .into_inner()
            .list)
    }

    /// Fetch changelog text for a specific registry tag.
    /// # Errors
    /// Fails if remote execution of `ota-update` tool failed, or on network IO errors
    pub async fn fetch_changelog(
        &mut self,
        reference: String,
        auth: Option<crate::RegistryAuth>,
        insecure: bool,
    ) -> anyhow::Result<String> {
        let request = pb::admin::RegistryChangelogRequest {
            reference,
            insecure,
            credentials: Self::registry_credentials(auth),
        };
        Ok(self
            .client
            .changelog(request)
            .await
            .rewrap_err()?
            .into_inner()
            .changelog)
    }

    /// Pull OTA update artifacts from a registry repository.
    /// # Errors
    /// Fails if remote execution of `ota-update` tool failed, or on network IO errors
    pub async fn pull_update(
        &mut self,
        reference: String,
        destination: String,
        auth: Option<crate::RegistryAuth>,
        insecure: bool,
    ) -> anyhow::Result<tonic::Streaming<pb::admin::RegistryPullResponse>> {
        let request = pb::admin::RegistryPullRequest {
            reference,
            destination,
            insecure,
            credentials: Self::registry_credentials(auth),
        };
        Ok(self.client.pull(request).await.rewrap_err()?.into_inner())
    }

    /// Install image on ghaf-host from a manifest path.
    /// # Errors
    /// Fails if remote execution of `ota-update` tool failed, or on network IO errors
    pub async fn image_install(
        &mut self,
        manifest: String,
    ) -> anyhow::Result<tonic::Streaming<pb::admin::ImageInstallResponse>> {
        let request = pb::admin::ImageInstallRequest { manifest };
        Ok(self
            .client
            .image_install(request)
            .await
            .rewrap_err()?
            .into_inner())
    }

    /// Install choosed pinned release from cachix.
    /// # Errors
    /// Fails if remote execution of `ota-update` tool failed, or on network IO errors
    pub async fn install_cachix(
        &mut self,
        cachix_request: pb::admin::Cachix,
    ) -> anyhow::Result<tonic::Streaming<pb::admin::SetGenerationResponse>> {
        Ok(self
            .client
            .install_cachix(cachix_request)
            .await
            .rewrap_err()?
            .into_inner())
    }

    fn registry_credentials(
        auth: Option<crate::RegistryAuth>,
    ) -> Option<pb::admin::RegistryCredentials> {
        let auth = auth?;
        let auth = match auth {
            crate::RegistryAuth::Basic { username, password } => {
                pb::registry_credentials::Auth::Basic(pb::admin::RegistryBasicAuth {
                    username,
                    password,
                })
            }
            crate::RegistryAuth::Bearer { token } => {
                pb::registry_credentials::Auth::Bearer(pb::admin::RegistryBearerAuth { token })
            }
        };
        Some(pb::admin::RegistryCredentials { auth: Some(auth) })
    }
}
