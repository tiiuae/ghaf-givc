use super::x509::SecurityInfo;
use tonic::{Request, Status};

fn security_info_from_request(req: &Request<()>) -> Result<SecurityInfo, Status> {
    if let Some(certs) = req.peer_certs() {
        for each in certs.iter() {
            if let Ok(info) = SecurityInfo::try_from(each.get_ref()) {
                return Ok(info);
            }
        }
        Err(Status::unauthenticated("Can't determine certificate"))
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
        .map(|si| si.check_hostname(hostname))
        .unwrap_or(false);
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
        .map(|si| hostnames.iter().any(|hostname| si.check_hostname(hostname)))
        .unwrap_or(false);
    if permit {
        Ok(())
    } else {
        Err(Status::permission_denied(format!(
            "Permissions for {} not confirmed by certificate",
            hostnames.join(", ")
        )))
    }
}
