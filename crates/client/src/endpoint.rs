// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use hyper_util::rt::TokioIo;
use spiffe::{TrustDomain, X509Source, bundle::BundleSource};
use tokio::net::UnixStream;
use tokio_vsock::{VsockAddr, VsockStream};
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Identity, ServerTlsConfig};
use tonic::transport::{Endpoint, Uri};
use tower::service_fn;
use tracing::info;

use givc_common::address::EndpointAddress;
use givc_common::types::TransportConfig;

#[derive(Debug, Clone)]
pub enum TlsMode {
    Static {
        ca_cert_file_path: PathBuf,
        cert_file_path: PathBuf,
        key_file_path: PathBuf,
    },
    Spiffe {
        endpoint: Option<String>,
        trust_domain: TrustDomain,
    },
}

#[derive(Debug, Clone)]
pub struct TlsConfig {
    pub mode: TlsMode,

    // For servers is None, and we read dnsName from cert. For client -- it must supplied.
    pub tls_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EndpointConfig {
    pub transport: TransportConfig,
    pub tls: Option<TlsConfig>,
}

impl TlsConfig {
    /// # Errors
    /// Fails if unable to read TLS certs/keys
    pub async fn client_config(&self) -> anyhow::Result<ClientTlsConfig> {
        match &self.mode {
            TlsMode::Static {
                ca_cert_file_path,
                cert_file_path,
                key_file_path,
            } => {
                let pem = tokio::fs::read(ca_cert_file_path).await?;
                let ca = Certificate::from_pem(pem);

                let client_cert = tokio::fs::read(cert_file_path).await?;
                let client_key = tokio::fs::read(key_file_path).await?;
                let client_identity = Identity::from_pem(client_cert, client_key);
                let tls_name = self.tls_name.as_deref().context("Missing TLS name")?;
                info!("Using TLS name: {tls_name}");
                Ok(ClientTlsConfig::new()
                    .ca_certificate(ca)
                    .domain_name(tls_name)
                    .identity(client_identity))
            }
            TlsMode::Spiffe {
                endpoint,
                trust_domain,
            } => {
                let source = self.spiffe_source(endpoint.as_deref()).await?;
                let (cert_pem, key_pem) = spiffe_svid_pem(&source)?;
                let ca_pem = self.spiffe_bundle_pem(&source, trust_domain)?;
                let tls_name = self
                    .tls_name
                    .clone()
                    .unwrap_or_else(|| trust_domain.to_string());

                Ok(ClientTlsConfig::new()
                    .ca_certificate(Certificate::from_pem(ca_pem))
                    .domain_name(tls_name)
                    .identity(Identity::from_pem(cert_pem, key_pem)))
            }
        }
    }

    /// # Errors
    /// Fails if unable to read TLS certs/keys
    pub async fn server_config(&self) -> anyhow::Result<ServerTlsConfig> {
        match &self.mode {
            TlsMode::Static {
                ca_cert_file_path,
                cert_file_path,
                key_file_path,
            } => {
                let ca_pem = tokio::fs::read(ca_cert_file_path).await?;
                let cert = tokio::fs::read(cert_file_path).await?;
                let key = tokio::fs::read(key_file_path).await?;
                let identity = Identity::from_pem(cert, key);
                let ca = Certificate::from_pem(ca_pem);
                let config = ServerTlsConfig::new().identity(identity).client_ca_root(ca);
                Ok(config)
            }
            TlsMode::Spiffe {
                endpoint,
                trust_domain,
            } => {
                let source = self.spiffe_source(endpoint.as_deref()).await?;
                let (cert_pem, key_pem) = spiffe_svid_pem(&source)?;
                let ca_pem = self.spiffe_bundle_pem(&source, trust_domain)?;
                Ok(ServerTlsConfig::new()
                    .identity(Identity::from_pem(cert_pem, key_pem))
                    .client_ca_root(Certificate::from_pem(ca_pem)))
            }
        }
    }

    async fn spiffe_source(&self, endpoint: Option<&str>) -> anyhow::Result<X509Source> {
        let source = if let Some(endpoint) = endpoint {
            X509Source::builder().endpoint(endpoint).build().await?
        } else {
            X509Source::new().await?
        };
        Ok(source)
    }

    fn spiffe_bundle_pem(
        &self,
        source: &X509Source,
        trust_domain: &TrustDomain,
    ) -> anyhow::Result<Vec<u8>> {
        let bundle = source
            .bundle_for_trust_domain(trust_domain)?
            .context("Missing SPIFFE trust bundle for trust domain")?;
        let mut pem_bundle = Vec::new();
        for cert in bundle.authorities() {
            let encoded = pem::encode(&pem::Pem::new("CERTIFICATE", cert.as_bytes()));
            pem_bundle.extend_from_slice(encoded.as_bytes());
        }
        Ok(pem_bundle)
    }
}

fn spiffe_svid_pem(source: &X509Source) -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
    let svid = source.svid()?;
    let mut cert_pem = Vec::new();
    for cert in svid.cert_chain() {
        let encoded = pem::encode(&pem::Pem::new("CERTIFICATE", cert.as_bytes()));
        cert_pem.extend_from_slice(encoded.as_bytes());
    }
    let key_pem = pem::encode(&pem::Pem::new("PRIVATE KEY", svid.private_key().as_bytes()));
    Ok((cert_pem, key_pem.into_bytes()))
}

fn transport_config_to_url(ea: &EndpointAddress, with_tls: bool) -> String {
    let scheme = if with_tls { "https" } else { "http" };
    match ea {
        EndpointAddress::Tcp { addr, port } => format!("{scheme}://{addr}:{port}"),
        _ => format!("{scheme}://[::]:443"), // Bogus url, to make tonic connector happy
    }
}

async fn connect_unix_socket(endpoint: Endpoint, path: &String) -> anyhow::Result<Channel> {
    let path = Arc::new(path.to_owned());
    let ch = endpoint
        .connect_with_connector(service_fn(move |_: Uri| {
            let path = path.clone();
            async move { UnixStream::connect(path.as_ref()).await.map(TokioIo::new) }
        }))
        .await?;
    Ok(ch)
}

async fn connect_vsock_socket(endpoint: Endpoint, vs: VsockAddr) -> anyhow::Result<Channel> {
    let ch = endpoint
        .connect_with_connector(service_fn(move |_: Uri| async move {
            VsockStream::connect(vs).await.map(TokioIo::new)
        }))
        .await?;
    Ok(ch)
}

impl EndpointConfig {
    /// Connect to configured endpoint
    /// # Errors
    /// Fails if connection failed
    pub async fn connect(&self) -> anyhow::Result<Channel> {
        let url = transport_config_to_url(&self.transport.address, self.tls.is_some());
        info!("Connecting to {url}, TLS name {:?}", &self.tls);
        let mut endpoint = Endpoint::try_from(url.clone())?
            .connect_timeout(Duration::from_millis(300))
            .concurrency_limit(30);
        if let Some(tls) = &self.tls {
            endpoint = endpoint.tls_config(tls.client_config().await?)?;
        }
        let channel = match &self.transport.address {
            EndpointAddress::Tcp { .. } => endpoint
                .connect()
                .await
                .with_context(|| format!("Connecting TCP {url} with {:?}", self.tls))?,
            EndpointAddress::Unix(unix) => connect_unix_socket(endpoint, unix).await?,
            EndpointAddress::Abstract(abs) => connect_unix_socket(endpoint, abs).await?,
            EndpointAddress::Vsock(vs) => connect_vsock_socket(endpoint, *vs).await?,
        };
        Ok(channel)
    }
}
