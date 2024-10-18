// This module contain literal translations of types from internal/pkgs/types/types.go
// Some of them would be rewritten, replaced, or even removed
use crate::pb;
use anyhow::anyhow;
use std::convert::TryFrom;

use givc_common::query::*;
use givc_common::types::*;

#[derive(Debug, Clone, PartialEq)]
pub enum Placement {
    // Service is a `givc-agent` and could be directly connected
    Endpoint(EndpointEntry),

    // Service or application managed by specified agent
    Managed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct RegistryEntry {
    pub name: String,
    pub r#type: UnitType,
    pub status: UnitStatus,
    pub placement: Placement,
    pub watch: bool,
}

impl RegistryEntry {
    pub fn agent(&self) -> anyhow::Result<&EndpointEntry> {
        match &self.placement {
            Placement::Endpoint(endpoint) => Ok(endpoint),
            Placement::Managed(by) => Err(anyhow!(
                "Agent endpoint {} is managed by {}!",
                self.name,
                by
            )),
        }
    }
}

#[cfg(test)]
impl RegistryEntry {
    pub fn dummy(n: String) -> Self {
        use givc_common::address::EndpointAddress;
        Self {
            name: n,
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
            placement: Placement::Endpoint(EndpointEntry {
                address: EndpointAddress::Tcp {
                    addr: "127.0.0.1".to_string(),
                    port: 42,
                },
                tls_name: "bogus".to_string(),
            }),
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
        // FIXME: We currently ignore `req.parent`, what we should do if we got both parent and endpoint
        // Protocol very inconsistent here
        Ok(Self {
            name: req.name,
            status,
            watch,
            r#type: ty,
            placement: Placement::Endpoint(endpoint),
        })
    }
}

impl From<RegistryEntry> for QueryResult {
    fn from(val: RegistryEntry) -> Self {
        let status = if val.status.is_running() {
            VMStatus::Running
        } else {
            VMStatus::PoweredOff
        };
        QueryResult {
            name: val.name,
            description: val.status.description,
            status,
            trust_level: TrustLevel::default(),
        }
    }
}
