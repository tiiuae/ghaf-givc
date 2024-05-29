// This module contain literal translations of types from internal/pkgs/types/types.go
// Some of them would be rewritten, replaced, or even removed
use std::convert::{Into, TryFrom};

#[derive(Debug, Clone, PartialEq)]
pub struct UnitType {
    pub vm: VmType,
    pub service: ServiceType,
}

#[derive(Debug, Clone, PartialEq)]
enum VmType {
    Host,
    AdmVM,
    SysVM,
    AppVM,
}

#[derive(Debug, Clone, PartialEq)]
enum ServiceType {
    Mgr,
    Svc,
    App,
    VM,
}

// Go version use u32 for UnitType, where we use more sophisticated types
// Let provide decode with error handling (we can get value overflow from wire)
impl TryFrom<u32> for UnitType {
    type Error = String;
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
            n => Err(format!("Unknown u32 value for UnitType: {n}")),
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

#[derive(Debug, Clone)]
pub struct UnitStatus {
    pub name: String,
    pub description: String,
    pub load_state: String,
    pub active_state: String,
    pub sub_state: String,
    pub path: String, // FIXME: PathBuf?
}

#[derive(Debug, Clone)]
pub struct TlsConfig {
    ca_cert_file_path: String,
    cert_file_path: String,
    key_file_path: String,
}

#[derive(Debug, Clone)]
pub struct TransportConfig {
    pub address: String,
    pub port: u16,
    pub protocol: String,
    pub tls_config: TlsConfig,
}

#[derive(Debug, Clone)]
pub struct EndpointConfig {
    pub name: String,
    pub transport: TransportConfig,
    pub services: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct EndpointEntry {
    pub name: String,
    pub protocol: String,
    pub address: String,
    pub port: String,
    pub with_tls: bool,
}

#[derive(Debug, Clone)]
pub struct RegistryEntry {
    pub name: String,
    pub parent: String,
    pub r#type: UnitType,
    pub status: UnitStatus,
    pub endpoint: EndpointEntry,
    pub watch: bool,
}
