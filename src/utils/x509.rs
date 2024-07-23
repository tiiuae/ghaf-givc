use std::convert::TryFrom;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use x509_parser::prelude::*;

#[derive(Debug)]
pub struct SecurityInfo {
    enabled: bool,
    dns_names: Vec<String>,
    ip_addrs: Vec<IpAddr>,
}

impl SecurityInfo {
    fn new() -> Self {
        Self {
            enabled: true,
            dns_names: Vec::new(),
            ip_addrs: Vec::new(),
        }
    }

    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Self::new()
        }
    }

    pub fn check_address(&self, ia: &IpAddr) -> bool {
        if self.enabled {
            self.ip_addrs.iter().any(|a| a == ia)
        } else {
            true
        }
    }

    pub fn check_hostname(&self, hostname: &str) -> bool {
        if self.enabled {
            self.dns_names.iter().any(|hn| hostname == hn)
        } else {
            true
        }
    }
}

impl TryFrom<&[u8]> for SecurityInfo {
    type Error = X509Error;
    fn try_from(cert: &[u8]) -> Result<Self, Self::Error> {
        let mut this = Self::new();
        let (_, x509) = parse_x509_certificate(cert)?;
        for ext in x509.extensions() {
            if let ParsedExtension::SubjectAlternativeName(san) = ext.parsed_extension() {
                for name in &san.general_names {
                    match name {
                        GeneralName::DNSName(s) => this.dns_names.push(s.to_string()),
                        GeneralName::IPAddress(b) if b.len() == 4 => {
                            let b = <[u8; 4]>::try_from(*b).unwrap();
                            let ip = IpAddr::V4(Ipv4Addr::from(b));
                            this.ip_addrs.push(ip)
                        }
                        GeneralName::IPAddress(b) if b.len() == 16 => {
                            let b = <[u8; 16]>::try_from(*b).unwrap();
                            let ip = IpAddr::V6(Ipv6Addr::from(b));
                            this.ip_addrs.push(ip)
                        }
                        _ => (),
                    }
                }
            }
        }
        Ok(this)
    }
}
