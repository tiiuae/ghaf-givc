// This module contain literal translations of types from internal/pkgs/types/types.go
// Some of them would be rewritten, replaced, or even removed
use crate::pb;
use anyhow::{anyhow, bail};
use std::convert::{Into, TryFrom};

#[derive(Debug, Clone, PartialEq)]
pub struct UnitType {
    pub vm: VmType,
    pub service: ServiceType,
}

#[derive(Debug, Clone, PartialEq)]
pub enum VmType {
    Host,
    AdmVM,
    SysVM,
    AppVM,
}

#[derive(Debug, Clone, PartialEq)]
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
    pub protocol: String,
    pub address: String,
    pub port: u16,
}

pub type TransportConfig = EndpointEntry;

impl TryFrom<pb::TransportConfig> for EndpointEntry {
    type Error = anyhow::Error;
    fn try_from(tc: pb::TransportConfig) -> Result<Self, Self::Error> {
        Ok(Self {
            protocol: tc.protocol,
            address: tc.address,
            port: tc.port.parse()?,
        })
    }
}

impl Into<pb::TransportConfig> for EndpointEntry {
    fn into(self) -> pb::TransportConfig {
        pb::TransportConfig {
            protocol: self.protocol,
            address: self.address,
            port: self.port.to_string(),
            name: String::from("unused"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RegistryEntry {
    pub name: String,
    pub parent: String,
    pub r#type: UnitType,
    pub status: UnitStatus,
    pub endpoint: EndpointEntry,
    pub watch: bool,
}

#[cfg(test)]
impl RegistryEntry {
    pub fn dummy(n: String) -> Self {
        Self {
            name: n,
            parent: "bogus".to_string(),
            r#type: UnitType {
                vm: VmType::AppVM,
                service: ServiceType::App,
            },
            status: UnitStatus {
                name: "systemd-servicename.service".to_string(),
                description: "bogus".to_string(),
                active_state: "active".to_string(),
                load_state: "loaded".to_string(),
                sub_state: "bogus".to_string(),
                path: "bogus".to_string(),
            },
            endpoint: EndpointEntry {
                protocol: "bogus".to_string(),
                address: "127.0.0.1".to_string(),
                port: 42,
            },
            watch: true,
        }
    }
}

impl TryFrom<pb::RegistryRequest> for RegistryEntry {
    type Error = anyhow::Error;
    fn try_from(req: pb::RegistryRequest) -> Result<Self, Self::Error> {
        let ty = UnitType::try_from(req.r#type)?;
        let status = req
            .state
            .ok_or(anyhow!("status missing"))
            .and_then(UnitStatus::try_from)?;
        let endpoint = req
            .transport
            .ok_or(anyhow!("endpoint missing"))
            .and_then(EndpointEntry::try_from)?;
        let watch = ty.service == ServiceType::Mgr;
        Ok(Self {
            name: req.name,
            parent: req.parent,
            status: status,
            watch: watch,
            r#type: ty,
            endpoint: endpoint,
        })
    }
}
