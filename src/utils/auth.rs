use super::x509::SecurityInfo;
use tonic::{Request, Status};

fn security_info_from_request(req: &Request<()>) -> Result<SecurityInfo, Status> {
    if let Some(certs) = req.peer_certs() {
        certs
            .iter()
            .find_map(|cert| SecurityInfo::try_from(cert.get_ref()).ok())
            .ok_or(Status::unauthenticated("Can't determinate certificace"))
    } else {
        Err(Status::unauthenticated("No valid certificate"))
    }
}

pub fn auth_interceptor(mut req: Request<()>) -> Result<Request<()>, Status> {
    let info = security_info_from_request(&req)?;
    if let Some(addr) = req.remote_addr() {
        if info.check_address(&addr.ip()) {
            req.extensions_mut().insert(info);
            Ok(req)
        } else {
            Err(Status::permission_denied(format!(
                "Address {addr} mismatched with address in certificate"
            )))
        }
    } else {
        Err(Status::unauthenticated("Can't determine IP address"))
    }
}

pub fn no_auth_interceptor(mut req: Request<()>) -> Result<Request<()>, Status> {
    req.extensions_mut().insert(SecurityInfo::disabled());
    Ok(req)
}

pub fn ensure_host<R>(req: Request<R>, hostname: &str) -> Result<(), Status> {
    let permit = req
        .extensions()
        .get::<SecurityInfo>()
        .is_some_and(|si| si.check_hostname(hostname));
    if permit {
        Ok(())
    } else {
        Err(Status::permission_denied(format!(
            "Permissions for {} not confirmed by certificate",
            hostname
        )))
    }
}

pub fn ensure_hosts<R>(req: Request<R>, hostnames: &Vec<&str>) -> Result<(), Status> {
    let permit = req
        .extensions()
        .get::<SecurityInfo>()
        .is_some_and(|si| hostnames.iter().any(|hostname| si.check_hostname(hostname)));
    if permit {
        Ok(())
    } else {
        Err(Status::permission_denied(format!(
            "Permissions for {} not confirmed by certificate",
            hostnames.join(", ")
        )))
    }
}
