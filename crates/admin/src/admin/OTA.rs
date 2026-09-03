// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use crate::endpoint::EndpointConfig;
use crate::pb::update::{
    self, AvailableUpdate, Cachix, Generation, ImageInstallRequest, ImageInstallResponse,
    RegistryChangelogRequest, RegistryCredentials, RegistryDiscoverRequest, RegistryPullRequest,
    RegistryPullResponse, SetGenerationResponse,
};
use crate::utils::tonic::{Stream, WrapError};
use givc_client::stream::check_trailers;
use givc_client::update::UpdateClient;

pub(crate) type SetGenerationStream = Stream<SetGenerationResponse>;
pub(crate) type ImageInstallStream = Stream<ImageInstallResponse>;
pub(crate) type PullStream = Stream<RegistryPullResponse>;

#[allow(clippy::upper_case_acronyms)]
pub(crate) struct OTA {
    endpoint: EndpointConfig,
}

impl OTA {
    pub(crate) fn new(endpoint: EndpointConfig) -> Self {
        Self { endpoint }
    }

    pub async fn list(&self) -> anyhow::Result<Vec<Generation>> {
        let mut client = UpdateClient::connect(self.endpoint.clone()).await?;
        let gens = client.list_generations().await?;
        Ok(gens)
    }

    pub async fn discover(
        &self,
        request: RegistryDiscoverRequest,
    ) -> anyhow::Result<Vec<AvailableUpdate>> {
        let mut client = UpdateClient::connect(self.endpoint.clone()).await?;
        let updates = client
            .discover_updates(
                request.reference,
                Self::registry_auth(request.credentials),
                request.insecure,
            )
            .await?;
        Ok(updates)
    }

    pub async fn changelog(&self, request: RegistryChangelogRequest) -> anyhow::Result<String> {
        let mut client = UpdateClient::connect(self.endpoint.clone()).await?;
        client
            .fetch_changelog(
                request.reference,
                Self::registry_auth(request.credentials),
                request.insecure,
            )
            .await
    }

    pub async fn pull(&self, request: RegistryPullRequest) -> anyhow::Result<PullStream> {
        let mut client = UpdateClient::connect(self.endpoint.clone()).await?;
        let mut stream = client
            .pull_update(
                request.reference,
                request.destination,
                Self::registry_auth(request.credentials),
                request.insecure,
            )
            .await?;
        let passthrough = async_fn_stream::try_fn_stream(async move |emitter| {
            while let Some(message) = stream.message().await? {
                emitter.emit(message).await;
            }
            check_trailers(stream).await.wrap_error()?;
            Ok(())
        });
        Ok(Box::pin(passthrough) as PullStream)
    }

    pub async fn image_install(
        &self,
        request: ImageInstallRequest,
    ) -> anyhow::Result<ImageInstallStream> {
        let mut client = UpdateClient::connect(self.endpoint.clone()).await?;
        let mut stream = client.image_install(request.manifest).await?;
        let passthrough = async_fn_stream::try_fn_stream(async move |emitter| {
            while let Some(message) = stream.message().await? {
                emitter.emit(message).await;
            }
            check_trailers(stream).await.wrap_error()?;
            Ok(())
        });
        Ok(Box::pin(passthrough) as ImageInstallStream)
    }

    pub async fn install_via_cachix(
        &self,
        cachix_request: Cachix,
    ) -> anyhow::Result<Stream<SetGenerationResponse>> {
        let mut client = UpdateClient::connect(self.endpoint.clone()).await?;
        let mut stream = client.install_cachix(cachix_request).await?;
        let passthrough = async_fn_stream::try_fn_stream(async move |emitter| {
            while let Some(message) = stream.message().await? {
                emitter.emit(message).await;
            }
            check_trailers(stream).await.wrap_error()?;
            Ok(())
        });
        Ok(Box::pin(passthrough) as SetGenerationStream)
    }

    fn registry_auth(
        credentials: Option<RegistryCredentials>,
    ) -> Option<givc_client::RegistryAuth> {
        let credentials = credentials?;
        match credentials.auth {
            Some(update::registry_credentials::Auth::Basic(basic)) => {
                Some(givc_client::RegistryAuth::Basic {
                    username: basic.username,
                    password: basic.password,
                })
            }
            Some(update::registry_credentials::Auth::Bearer(bearer)) => {
                Some(givc_client::RegistryAuth::Bearer {
                    token: bearer.token,
                })
            }
            None => None,
        }
    }
}
