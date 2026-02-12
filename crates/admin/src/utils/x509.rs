// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::convert::TryFrom;
use std::net::IpAddr;
use x509_parser::prelude::*;

#[derive(Clone, Debug)]
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

    #[must_use]
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Self::new()
        }
    }

    #[must_use]
    pub fn check_address(&self, ia: &IpAddr) -> bool {
        !self.enabled || self.ip_addrs.iter().any(|a| a == ia)
    }

    #[must_use]
    pub fn check_hostname(&self, hostname: &str) -> bool {
        !self.enabled || self.dns_names.iter().any(|hn| hostname == hn)
    }

    #[must_use]
    pub fn hostname(self) -> Option<String> {
        self.dns_names.into_iter().next()
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
                        GeneralName::DNSName(s) => this.dns_names.push((*s).to_string()),
                        GeneralName::IPAddress(b) if b.len() == 4 => {
                            let b = <[u8; 4]>::try_from(*b).unwrap();
                            let ip = IpAddr::from(b);
                            this.ip_addrs.push(ip);
                        }
                        GeneralName::IPAddress(b) if b.len() == 16 => {
                            let b = <[u8; 16]>::try_from(*b).unwrap();
                            let ip = IpAddr::from(b);
                            this.ip_addrs.push(ip);
                        }
                        _ => (),
                    }
                }
            }
        }
        Ok(this)
    }
}
