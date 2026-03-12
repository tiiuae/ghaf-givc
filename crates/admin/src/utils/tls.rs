// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use crate::endpoint::{TlsConfig, TlsMode as EndpointTlsMode};
use anyhow::Context;
use clap::{Args, ValueEnum};
use spiffe::TrustDomain;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CliTlsMode {
    Static,
    Spiffe,
    None,
}

#[derive(Debug, Clone, Args)]
pub struct CliTlsOptions {
    #[arg(long, env = "TLS_MODE", value_enum, default_value_t = CliTlsMode::Static)]
    pub tls_mode: CliTlsMode,

    #[arg(long, env = "CA_CERT")]
    pub ca_cert: Option<PathBuf>,

    #[arg(long, env = "HOST_CERT")]
    pub host_cert: Option<PathBuf>,

    #[arg(long, env = "HOST_KEY")]
    pub host_key: Option<PathBuf>,

    #[arg(long, env = "SPIFFE_ENDPOINT")]
    pub spiffe_endpoint: Option<String>,

    #[arg(long, env = "TRUST_DOMAIN")]
    pub trust_domain: Option<TrustDomain>,
}

impl CliTlsOptions {
    pub fn into_endpoint_tls_mode(self) -> anyhow::Result<Option<EndpointTlsMode>> {
        match self.tls_mode {
            CliTlsMode::None => Ok(None),
            CliTlsMode::Static => Ok(Some(EndpointTlsMode::Static {
                ca_cert_file_path: self.ca_cert.context("ca cert is required")?,
                cert_file_path: self.host_cert.context("host cert is required")?,
                key_file_path: self.host_key.context("host key is required")?,
            })),
            CliTlsMode::Spiffe => Ok(Some(EndpointTlsMode::Spiffe {
                endpoint: self.spiffe_endpoint,
                trust_domain: self.trust_domain.context("trust domain is required")?,
            })),
        }
    }

    pub fn into_server_tls_config(self) -> anyhow::Result<Option<TlsConfig>> {
        Ok(self.into_endpoint_tls_mode()?.map(|mode| TlsConfig {
            mode,
            tls_name: None,
        }))
    }

    pub fn into_client_tls_config(self, tls_name: String) -> anyhow::Result<Option<TlsConfig>> {
        Ok(self.into_endpoint_tls_mode()?.map(|mode| TlsConfig {
            mode,
            tls_name: Some(tls_name),
        }))
    }
}
