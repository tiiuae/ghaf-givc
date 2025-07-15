use super::x509::SecurityInfo;
use tonic::{Request, Status};

/// # Errors
/// Return `Err(tonic::Status)` if IP in request have no certificate
fn security_info_from_request(req: &Request<()>) -> Result<SecurityInfo, Box<Status>> {
    req.peer_certs()
        .ok_or(Status::unauthenticated("No valid certificate"))?
        .iter()
        .find_map(|cert| SecurityInfo::try_from(cert.as_ref()).ok())
        .ok_or(Status::unauthenticated("Can't determinate certificace").into())
}

/// # Errors
/// Return `Err(tonic::Status)` if IP in request headers mismatch IP in certificate
pub fn auth_interceptor(mut req: Request<()>) -> Result<Request<()>, Box<Status>> {
    let info = security_info_from_request(&req)?;
    let addr = req
        .remote_addr()
        .ok_or(Status::unauthenticated("Can't determine IP address"))?;
    if info.check_address(&addr.ip()) {
        req.extensions_mut().insert(info);
        Ok(req)
    } else {
        Err(Status::permission_denied(format!(
            "Address {addr} mismatched with address in certificate"
        ))
        .into())
    }
}

/// # Errors
/// This function always success and return `Ok()`
pub fn no_auth_interceptor(mut req: Request<()>) -> Result<Request<()>, Box<Status>> {
    req.extensions_mut().insert(SecurityInfo::disabled());
    Ok(req)
}

/// # Errors
/// This function fails if hostname permission not confirmed by certificate
pub fn ensure_host<R>(req: &Request<R>, hostname: &str) -> Result<(), Box<Status>> {
    req.extensions()
        .get::<SecurityInfo>()
        .is_some_and(|si| si.check_hostname(hostname))
        .then_some(())
        .ok_or_else(|| {
            Status::permission_denied(format!(
                "Permissions for {hostname} not confirmed by certificate"
            ))
            .into()
        })
}

/// # Errors
/// This function fails if hostname permission not confirmed by certificate
pub fn ensure_hosts<R>(req: &Request<R>, hostnames: &[&str]) -> Result<(), Box<Status>> {
    req.extensions()
        .get::<SecurityInfo>()
        .is_some_and(|si| hostnames.iter().any(|hostname| si.check_hostname(hostname)))
        .then_some(())
        .ok_or_else(|| {
            Status::permission_denied(format!(
                "Permissions for {} not confirmed by certificate",
                hostnames.join(", ")
            ))
            .into()
        })
}
