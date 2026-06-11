// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use tonic::transport::{Channel, Endpoint};
use tracing::debug;

use givc_common::address::EndpointAddress;
use givc_common::authn::TlsConfig;
use givc_common::tls_stream::connect_endpoint;
use givc_common::types::TransportConfig;

#[derive(Debug, Clone)]
pub struct EndpointConfig {
    pub transport: TransportConfig,
    pub tls: Option<TlsConfig>,
}

fn transport_config_to_url(ea: &EndpointAddress) -> String {
    // We always use http to trick Tonic; our custom connector handles TLS transparently.
    let scheme = "http";
    match ea {
        EndpointAddress::Tcp { addr, port } => format!("{scheme}://{addr}:{port}"),
        _ => format!("{scheme}://[::]:443"), // Bogus url, to make tonic connector happy
    }
}

impl EndpointConfig {
    /// Connect to configured endpoint
    /// # Errors
    /// Fails if connection failed
    pub async fn connect(&self) -> anyhow::Result<Channel> {
        let url = transport_config_to_url(&self.transport.address);
        debug!("Connecting to {url}, TLS: {:?}", &self.tls);
        let endpoint = Endpoint::try_from(url.clone())?
            .connect_timeout(Duration::from_millis(300))
            .concurrency_limit(30);

        let mut tls_clone = self.tls.clone();
        let rustls_config = if let Some(tls) = &mut tls_clone {
            Some(Arc::new(
                tls.to_rustls_client()
                    .await
                    .map_err(|e| anyhow::anyhow!(e))?,
            ))
        } else {
            None
        };
        let domain = self.transport.tls_name.clone();

        let channel = connect_endpoint(endpoint, &self.transport.address, rustls_config, domain)
            .await
            .with_context(|| format!("Connecting {} with {:?}", url, self.tls))?;

        Ok(channel)
    }
}
