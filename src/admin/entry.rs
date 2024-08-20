// This module contain literal translations of types from internal/pkgs/types/types.go
// Some of them would be rewritten, replaced, or even removed
use crate::pb;
use anyhow::anyhow;
use std::convert::{Into, TryFrom};

use givc_common::query::*;
use givc_common::types::*;

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
                tls_name: "bogus".to_string(),
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

impl Into<QueryResult> for RegistryEntry {
    fn into(self) -> QueryResult {
        let status = if self.status.is_running() {
            VMStatus::Running
        } else {
            VMStatus::PoweredOff
        };
        QueryResult {
            name: self.name,
            description: self.status.description,
            status: status,
            trust_level: TrustLevel::default(),
        }
    }
}
