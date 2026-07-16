// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use anyhow::Context;

pub mod cli;
pub mod config;
pub mod ctap;
pub mod eventproxy;
pub mod exec;
pub mod hwid;
pub mod locale;
pub mod notifier;
pub mod policyadmin;
pub mod registration;
pub mod runtime;
pub mod service;
pub mod servicemanager;
pub mod socketproxy;
pub mod statsmanager;
pub mod wifimanager;

pub mod auth {
    use std::convert::TryFrom;
    use std::net::IpAddr;

    use http::Request as HttpRequest;
    use tonic::Status;
    use tonic::body::Body;
    use tonic::transport::server::{Connected, TlsConnectInfo};
    use tonic_middleware::RequestInterceptor;
    use tracing::debug;
    use x509_parser::prelude::*;

    type ListenerConnectInfo = <tokio::net::TcpStream as Connected>::ConnectInfo;

    #[derive(Clone, Debug)]
    pub struct SecurityInfo {
        enabled: bool,
        ip_addrs: Vec<IpAddr>,
    }

    impl SecurityInfo {
        fn new() -> Self {
            Self {
                enabled: true,
                ip_addrs: Vec::new(),
            }
        }

        #[must_use]
        pub fn disabled() -> Self {
            Self {
                enabled: false,
                ..Self::new()
            }
        }

        #[must_use]
        pub fn check_address(&self, ip: &IpAddr) -> bool {
            !self.enabled || self.ip_addrs.iter().any(|candidate| candidate == ip)
        }
    }

    impl TryFrom<&[u8]> for SecurityInfo {
        type Error = x509_parser::error::X509Error;

        fn try_from(cert: &[u8]) -> Result<Self, Self::Error> {
            let mut this = Self::new();
            let (_, x509) = parse_x509_certificate(cert)?;
            for ext in x509.extensions() {
                if let ParsedExtension::SubjectAlternativeName(san) = ext.parsed_extension() {
                    for name in &san.general_names {
                        if let GeneralName::IPAddress(b) = name {
                            match b.len() {
                                4 => {
                                    let b = <[u8; 4]>::try_from(*b).unwrap();
                                    this.ip_addrs.push(IpAddr::from(b));
                                }
                                16 => {
                                    let b = <[u8; 16]>::try_from(*b).unwrap();
                                    this.ip_addrs.push(IpAddr::from(b));
                                }
                                _ => (),
                            }
                        }
                    }
                }
            }
            Ok(this)
        }
    }

    #[derive(Clone)]
    pub struct Authenticator {
        pub use_tls: bool,
    }

    fn security_info_from_request<T>(req: &HttpRequest<T>) -> Result<SecurityInfo, Status> {
        req.extensions()
            .get::<TlsConnectInfo<ListenerConnectInfo>>()
            .and_then(TlsConnectInfo::peer_certs)
            .ok_or_else(|| Status::unauthenticated("No peer certificate"))?
            .iter()
            .find_map(|cert| SecurityInfo::try_from(cert.as_ref()).ok())
            .ok_or_else(|| Status::unauthenticated("Can't parse certificate"))
    }

    fn transport_info_from_request<T>(req: &HttpRequest<T>) -> Option<&ListenerConnectInfo> {
        req.extensions()
            .get::<TlsConnectInfo<ListenerConnectInfo>>()
            .map(TlsConnectInfo::get_ref)
    }

    #[tonic::async_trait]
    impl RequestInterceptor for Authenticator {
        async fn intercept(&self, mut req: HttpRequest<Body>) -> Result<HttpRequest<Body>, Status> {
            if self.use_tls {
                let security_info = security_info_from_request(&req)?;

                match transport_info_from_request(&req) {
                    Some(addr) => {
                        let ip = addr
                            .remote_addr()
                            .ok_or_else(|| Status::unauthenticated("Can't determine peer IP"))?
                            .ip();
                        if security_info.check_address(&ip) {
                            debug!("TCP: IP {ip} verified against certificate");
                            req.extensions_mut().insert(security_info);
                            Ok(req)
                        } else {
                            Err(Status::permission_denied(format!(
                                "IP {ip} not in certificate SAN"
                            )))
                        }
                    }
                    None => Err(Status::unauthenticated("No transport info")),
                }
            } else {
                req.extensions_mut().insert(SecurityInfo::disabled());
                Ok(req)
            }
        }
    }
}

/// Init logging.
///
/// # Errors
///
/// Will return `Err` if failed to initialize logging.
pub fn trace_init(debug: bool) -> anyhow::Result<()> {
    use std::env;
    use tracing::Level;
    use tracing_subscriber::{EnvFilter, Layer, layer::SubscriberExt};

    let env_filter = if debug && env::var("GIVC_LOG").is_err() {
        EnvFilter::from("debug")
    } else {
        EnvFilter::try_from_env("GIVC_LOG").unwrap_or_else(|_| EnvFilter::from("info"))
    };
    let is_debug_log_level = env_filter
        .max_level_hint()
        .map_or_else(|| false, |level| level >= Level::DEBUG);

    let output = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_target(is_debug_log_level)
        .with_file(is_debug_log_level)
        .with_line_number(is_debug_log_level)
        .with_thread_ids(is_debug_log_level);

    let output = if is_debug_log_level {
        output.pretty().boxed()
    } else {
        output.boxed()
    };

    if env::var("INVOCATION_ID").is_ok() {
        let journald = tracing_journald::layer()
            .map(|layer| layer.with_filter(env_filter.clone()).boxed())
            .unwrap_or(output.with_filter(env_filter).boxed());

        tracing::subscriber::set_global_default(tracing_subscriber::registry().with(journald))
            .context("tracing shouldn't already have been set up")?;
    } else {
        tracing::subscriber::set_global_default(
            tracing_subscriber::registry().with(output.with_filter(env_filter)),
        )
        .context("tracing shouldn't already have been set up")?;
    }

    Ok(())
}
