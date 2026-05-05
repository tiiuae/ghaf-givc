// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use super::x509::SecurityInfo;
use tonic::transport::server::{Connected, TlsConnectInfo};
use tonic::{Request, Status};
use tracing::debug;

/// Type alias for `tokio_listener`'s connection info.
type ListenerConnectInfo = <tokio_listener::Connection as Connected>::ConnectInfo;

use http::Request as HttpRequest;
use tonic::body::Body;
use tonic_middleware::RequestInterceptor;

/// Extract `SecurityInfo` from the peer certificate in the request.
fn security_info_from_request<T>(req: &HttpRequest<T>) -> Result<SecurityInfo, Status> {
    req.extensions()
        .get::<TlsConnectInfo<ListenerConnectInfo>>()
        .and_then(TlsConnectInfo::peer_certs)
        .ok_or_else(|| Status::unauthenticated("No peer certificate"))?
        .iter()
        .find_map(|cert| SecurityInfo::try_from(cert.as_ref()).ok())
        .ok_or_else(|| Status::unauthenticated("Can't parse certificate"))
}

/// Extract transport info from the request extensions.
fn transport_info_from_request<T>(req: &HttpRequest<T>) -> Option<&ListenerConnectInfo> {
    req.extensions()
        .get::<TlsConnectInfo<ListenerConnectInfo>>()
        .map(TlsConnectInfo::get_ref)
}

#[derive(Clone)]
pub struct AuthInterceptor {
    pub use_tls: bool,
}

/// Authentication interceptor that verifies the peer's identity.
///
/// **TCP**: Verifies peer IP matches an IP in their certificate's SAN.
/// **Vsock/Unix/Other**: Certificate validity only (TLS handshake). No IP check -
/// security relies on hypervisor isolation (vsock) or filesystem permissions (unix).
#[async_trait::async_trait]
impl RequestInterceptor for AuthInterceptor {
    async fn intercept(&self, mut req: HttpRequest<Body>) -> Result<HttpRequest<Body>, Status> {
        if self.use_tls {
            let security_info = security_info_from_request(&req)?;

            match transport_info_from_request(&req) {
                Some(ListenerConnectInfo::Tcp(tcp_info)) => {
                    let addr = tcp_info
                        .remote_addr()
                        .ok_or_else(|| Status::unauthenticated("Can't determine peer IP"))?;
                    let ip = addr.ip();
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
                Some(_) => {
                    debug!("Non-TCP transport: certificate valid, skipping IP check");
                    req.extensions_mut().insert(security_info);
                    Ok(req)
                }
                None => Err(Status::unauthenticated("No transport info")),
            }
        } else {
            req.extensions_mut().insert(SecurityInfo::disabled());
            Ok(req)
        }
    }
}

/// Verify the request is authorized for at least one of the given hostnames.
/// Used for host authorization based on certificate DNS SANs.
///
/// # Errors
/// Returns `Err(tonic::Status)` if no hostname matches
#[allow(dead_code)] // TODO: potentially use this for gRPC call credentials
pub fn ensure_host<R>(req: &Request<R>, hostnames: &[&str]) -> Result<(), Status> {
    req.extensions()
        .get::<SecurityInfo>()
        .is_some_and(|si| hostnames.iter().any(|hostname| si.check_hostname(hostname)))
        .then_some(())
        .ok_or_else(|| {
            Status::permission_denied(format!(
                "Permissions for {} not confirmed by certificate",
                hostnames.join(", ")
            ))
        })
}
