// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result};
use rustls::{ClientConfig, ServerConfig};
use rustls_pemfile::{certs, private_key};
use spiffe::{TrustDomain, X509Source};
use spiffe_rustls::{TrustDomainPolicy, authorizer, mtls_client, mtls_server};
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum AuthType {
    #[default]
    None,
    Legacy,
    Spire,
}

impl From<&str> for AuthType {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "spire" => AuthType::Spire,
            "legacy" => AuthType::Legacy,
            _ => AuthType::None,
        }
    }
}

/// Unified TLS configuration for both legacy (file-based) and SPIRE authentication.
#[derive(Debug)]
pub struct TlsConfig {
    pub auth_type: AuthType,

    root_store: Option<rustls::RootCertStore>,
    certs: Option<Vec<rustls::pki_types::CertificateDer<'static>>>,
    key: Option<rustls::pki_types::PrivateKeyDer<'static>>,

    pub trust_domain: Option<TrustDomain>,
    pub tls_name: Option<String>,
    svid_source: Option<X509Source>,

    client_config: Option<ClientConfig>,
    server_config: Option<ServerConfig>,
}

impl Clone for TlsConfig {
    fn clone(&self) -> Self {
        Self {
            auth_type: self.auth_type,
            root_store: self.root_store.clone(),
            certs: self.certs.clone(),
            key: self.key.as_ref().map(|k| k.clone_key()),
            trust_domain: self.trust_domain.clone(),
            tls_name: self.tls_name.clone(),
            svid_source: self.svid_source.clone(),
            client_config: self.client_config.clone(),
            server_config: self.server_config.clone(),
        }
    }
}

impl TlsConfig {
    pub async fn from_spire_agent(spire_socket: String, trust_domain: String) -> Result<Self> {
        let domain = TrustDomain::new(&trust_domain)
            .map_err(|e| anyhow::anyhow!("Invalid trust domain: {}", e))?;

        let endpoint_url = format!("unix://{}", spire_socket);
        let source = X509Source::builder()
            .endpoint(endpoint_url)
            .build()
            .await
            .context("failed to build SPIFFE X509 source")?;

        Ok(Self {
            auth_type: AuthType::Spire,
            root_store: None,
            certs: None,
            key: None,
            trust_domain: Some(domain),
            tls_name: None,
            svid_source: Some(source),
            client_config: None,
            server_config: None,
        })
    }

    pub fn from_certs_and_key(
        ca_cert: PathBuf,
        cert: PathBuf,
        key: PathBuf,
        tls_name: Option<String>,
    ) -> Result<Self> {
        // Load CA file into RootCertStore
        let mut store = rustls::RootCertStore::empty();
        let ca_file =
            File::open(&ca_cert).context(format!("failed to open CA file at {:?}", ca_cert))?;
        let mut ca_reader = BufReader::new(ca_file);
        for cert_res in certs(&mut ca_reader) {
            store
                .add(cert_res?)
                .map_err(|e| anyhow::anyhow!("failed to add cert to root store: {}", e))?;
        }

        // Load Certificate Chain
        let cert_file =
            File::open(&cert).context(format!("failed to open cert file at {:?}", cert))?;
        let mut cert_reader = BufReader::new(cert_file);
        let chain = certs(&mut cert_reader).collect::<Result<Vec<_>, _>>()?;

        // Load Private Key
        let key_file = File::open(&key).context(format!("failed to open key file at {:?}", key))?;
        let mut key_reader = BufReader::new(key_file);
        let k = private_key(&mut key_reader)?
            .ok_or_else(|| anyhow::anyhow!("no private key found in {:?}", key))?;

        Ok(Self {
            auth_type: AuthType::Legacy,
            root_store: Some(store),
            certs: Some(chain),
            key: Some(k),
            trust_domain: None,
            tls_name: tls_name,
            svid_source: None,
            client_config: None,
            server_config: None,
        })
    }

    pub async fn to_rustls_client(&mut self) -> Result<ClientConfig> {
        if let Some(ref config) = self.client_config {
            return Ok(config.clone());
        }

        match self.auth_type {
            AuthType::Legacy => {
                let mut cfg = rustls::ClientConfig::builder()
                    .with_root_certificates(self.root_store.clone().unwrap())
                    .with_client_auth_cert(
                        self.certs.clone().unwrap(),
                        self.key.as_ref().unwrap().clone_key(),
                    )
                    .context("failed to build rustls client config")?;

                cfg.alpn_protocols = vec![b"h2".to_vec()]; // Required for Tonic/gRPC

                self.client_config = Some(cfg);
            }
            AuthType::Spire => {
                let cfg = mtls_client(self.svid_source.clone().unwrap())
                    .authorize(authorizer::trust_domains([self
                        .trust_domain
                        .clone()
                        .unwrap()])?)
                    .trust_domain_policy(TrustDomainPolicy::LocalOnly(
                        self.trust_domain.clone().unwrap(),
                    ))
                    .with_alpn_protocols(&[b"h2"]) // for gRPC/HTTP/2
                    .build()
                    .context("failed to build SPIRE mtls client config")?;
                self.client_config = Some(cfg);
            }
            _ => {
                return Err(anyhow::anyhow!("Unknown AuthType"));
            }
        }

        self.client_config
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Client configuration failed to initialize"))
    }

    pub async fn to_rustls_server(&mut self) -> Result<ServerConfig> {
        if let Some(ref config) = self.server_config {
            return Ok(config.clone());
        }

        match self.auth_type {
            AuthType::Legacy => {
                let verifier = rustls::server::WebPkiClientVerifier::builder(Arc::new(
                    self.root_store.clone().unwrap(),
                ))
                .build()
                .map_err(|e| anyhow::anyhow!("failed to build client cert verifier: {}", e))?;

                let mut cfg = ServerConfig::builder()
                    .with_client_cert_verifier(verifier)
                    .with_single_cert(
                        self.certs.clone().unwrap(),
                        self.key.as_ref().unwrap().clone_key(),
                    )
                    .context("failed to build rustls server config")?;

                cfg.alpn_protocols = vec![b"h2".to_vec()]; // Required for Tonic/gRPC
                self.server_config = Some(cfg);
            }
            AuthType::Spire => {
                let cfg = mtls_server(self.svid_source.clone().unwrap())
                    .authorize(authorizer::trust_domains([self
                        .trust_domain
                        .clone()
                        .unwrap()])?)
                    .trust_domain_policy(TrustDomainPolicy::LocalOnly(
                        self.trust_domain.clone().unwrap(),
                    ))
                    .with_alpn_protocols(&[b"h2"]) // for gRPC/HTTP/2
                    .build()
                    .context("failed to build SPIRE mtls server config")?;
                self.server_config = Some(cfg);
            }
            _ => {
                return Err(anyhow::anyhow!("Unknown AuthType"));
            }
        }

        self.server_config
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Server configuration failed to initialize"))
    }
}
