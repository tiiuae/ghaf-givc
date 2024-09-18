// This module contain literal translations of types from internal/pkgs/types/types.go
// Some of them would be rewritten, replaced, or even removed
use super::address::EndpointAddress;
use crate::pb;
use std::convert::{Into, TryFrom};

use anyhow::{anyhow, bail};
use tokio_vsock::VsockAddr;

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct UnitType {
    pub vm: VmType,
    pub service: ServiceType,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum VmType {
    Host,
    AdmVM,
    SysVM,
    AppVM,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum ServiceType {
    Mgr,
    Svc,
    App,
    VM,
}

// Go version use u32 for UnitType, where we use more sophisticated types
// Let provide decode with error handling (we can get value overflow from wire)
impl TryFrom<u32> for UnitType {
    type Error = anyhow::Error;
    fn try_from(value: u32) -> Result<Self, Self::Error> {
        use ServiceType::*;
        use VmType::*;
        match value {
            0 => Ok(UnitType {
                vm: Host,
                service: Mgr,
            }),
            1 => Ok(UnitType {
                vm: Host,
                service: Svc,
            }),
            2 => Ok(UnitType {
                vm: Host,
                service: App,
            }),
            3 => Ok(UnitType {
                vm: AdmVM,
                service: VM,
            }),
            4 => Ok(UnitType {
                vm: AdmVM,
                service: Mgr,
            }),
            5 => Ok(UnitType {
                vm: AdmVM,
                service: Svc,
            }),
            6 => Ok(UnitType {
                vm: AdmVM,
                service: App,
            }),
            7 => Ok(UnitType {
                vm: SysVM,
                service: VM,
            }),
            8 => Ok(UnitType {
                vm: SysVM,
                service: Mgr,
            }),
            9 => Ok(UnitType {
                vm: SysVM,
                service: Svc,
            }),
            10 => Ok(UnitType {
                vm: SysVM,
                service: App,
            }),
            11 => Ok(UnitType {
                vm: AppVM,
                service: VM,
            }),
            12 => Ok(UnitType {
                vm: AppVM,
                service: Mgr,
            }),
            13 => Ok(UnitType {
                vm: AppVM,
                service: Svc,
            }),
            14 => Ok(UnitType {
                vm: AppVM,
                service: App,
            }),
            n => bail!("Unknown u32 value for UnitType: {n}"),
        }
    }
}

// Go version use u32 for UnitType, where we use more sophisticated types
// Let provide encoding, converting is straight and no need error handling,
// so we can use just Into<u32> trait
// FIXME:  Combination of `UnitType{ vm: Host, service: VM}` is ILLEGAL!!!
//         Should we use TryInto, or fix type system?
impl Into<u32> for UnitType {
    fn into(self) -> u32 {
        use ServiceType::*;
        use VmType::*;
        match self.vm {
            Host => match self.service {
                Mgr => 0,
                Svc => 1,
                App => 2,
                VM => 100500,
            },
            AdmVM => match self.service {
                VM => 3,
                Mgr => 4,
                Svc => 5,
                App => 6,
            },
            SysVM => match self.service {
                VM => 7,
                Mgr => 8,
                Svc => 9,
                App => 10,
            },
            AppVM => match self.service {
                VM => 11,
                Mgr => 12,
                Svc => 13,
                App => 14,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct UnitStatus {
    pub name: String,
    pub description: String,
    pub load_state: String,
    pub active_state: String,
    pub sub_state: String,
    pub path: String, // FIXME: PathBuf?
}

impl UnitStatus {
    pub fn is_running(&self) -> bool {
        self.active_state == "active" && self.load_state == "loaded" && self.sub_state == "running"
    }
    pub fn is_exitted(&self) -> bool {
        self.active_state == "inactive"
            && self.load_state == "not-found"
            && self.sub_state == "dead"
    }
}

impl TryFrom<pb::UnitStatus> for UnitStatus {
    type Error = anyhow::Error;
    fn try_from(us: pb::UnitStatus) -> Result<Self, Self::Error> {
        Ok(Self {
            name: us.name,
            description: us.description,
            load_state: us.load_state,
            active_state: us.active_state,
            sub_state: "stub".into(),
            path: us.path,
        })
    }
}

impl Into<pb::UnitStatus> for UnitStatus {
    fn into(self) -> pb::UnitStatus {
        pb::UnitStatus {
            name: self.name,
            description: self.description,
            load_state: self.load_state,
            active_state: self.active_state,
            path: self.path,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EndpointEntry {
    pub address: EndpointAddress,
    pub tls_name: String,
}

pub type TransportConfig = EndpointEntry;

impl TryFrom<pb::TransportConfig> for EndpointEntry {
    type Error = anyhow::Error;
    fn try_from(tc: pb::TransportConfig) -> Result<Self, Self::Error> {
        let endpoint = match tc.protocol.as_str() {
            "tcp" => EndpointAddress::Tcp {
                addr: tc.address,
                port: tc.port.parse()?,
            },
            "unix" => EndpointAddress::Unix(tc.address),
            "abstract" => EndpointAddress::Abstract(tc.address),
            "vsock" => {
                EndpointAddress::Vsock(VsockAddr::new(tc.address.parse()?, tc.port.parse()?))
            }
            unknown => bail!("Unknown protocol: {unknown}"),
        };
        Ok(Self {
            address: endpoint,
            tls_name: tc.name,
        })
    }
}

impl Into<pb::TransportConfig> for EndpointEntry {
    fn into(self) -> pb::TransportConfig {
        match self.address {
            EndpointAddress::Tcp { addr, port } => pb::TransportConfig {
                protocol: "tcp".into(),
                address: addr,
                port: port.to_string(),
                name: self.tls_name,
            },
            EndpointAddress::Unix(unix) => pb::TransportConfig {
                protocol: "unix".into(),
                address: unix,
                port: "".into(),
                name: self.tls_name,
            },
            EndpointAddress::Abstract(abstr) => pb::TransportConfig {
                protocol: "abstract".into(),
                address: abstr,
                port: "".into(),
                name: self.tls_name,
            },
            EndpointAddress::Vsock(vs) => pb::TransportConfig {
                protocol: "vsock".into(),
                address: vs.cid().to_string(),
                port: vs.port().to_string(),
                name: self.tls_name,
            },
        }
    }
}
