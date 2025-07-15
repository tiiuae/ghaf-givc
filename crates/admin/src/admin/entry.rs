// This module contain literal translations of types from internal/pkgs/types/types.go
// Some of them would be rewritten, replaced, or even removed
use crate::pb;
use anyhow::{Context, anyhow};
use std::convert::TryFrom;

use crate::utils::naming::parse_vm_name;
use givc_common::query::{QueryResult, TrustLevel, VMStatus};
use givc_common::types::{EndpointEntry, ServiceType, UnitStatus, UnitType, VmType};

#[derive(Debug, Clone, PartialEq)]
pub enum Placement {
    // Service is a `givc-agent` and could be directly connected
    Endpoint { endpoint: EndpointEntry, vm: String },

    // Service or application managed by specified agent
    Managed { vm: String, by: String },

    // Running on host
    Host,
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
    #[must_use]
    pub(crate) fn agent_name(&self) -> Option<&str> {
        match &self.placement {
            Placement::Endpoint { .. } => Some(&self.name),
            Placement::Managed { by, .. } => Some(by),
            Placement::Host => None,
        }
    }

    #[must_use]
    pub(crate) fn vm_name(&self) -> Option<&str> {
        match (self.r#type.service, &self.placement) {
            (ServiceType::VM, _) => parse_vm_name(&self.name),
            (_, Placement::Endpoint { vm, .. } | Placement::Managed { vm, .. }) => Some(vm),
            (_, Placement::Host) => None,
        }
    }

    pub(crate) fn agent(&self) -> anyhow::Result<&EndpointEntry> {
        match &self.placement {
            Placement::Endpoint { endpoint, .. } => Ok(endpoint),
            Placement::Managed { by, .. } => Err(anyhow!(
                "Agent endpoint {} is managed by {}!",
                self.name,
                by
            )),
            Placement::Host => Err(anyhow!("Its a host!")),
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
                freezer_state: "bogus".to_string(),
            },
            placement: Placement::Endpoint {
                endpoint: EndpointEntry {
                    address: EndpointAddress::Tcp {
                        addr: "127.0.0.1".to_string(),
                        port: 42,
                    },
                    tls_name: "bogus".to_string(),
                },
                vm: "bogus".into(),
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
            .context("status missing")
            .and_then(UnitStatus::try_from)?;
        let endpoint = req
            .transport
            .context("endpoint missing")
            .and_then(EndpointEntry::try_from)?;
        let watch = (ty.service == ServiceType::Mgr) || (ty.vm == VmType::AppVM);
        Ok(Self {
            name: req.name,
            status,
            watch,
            r#type: ty,
            placement: Placement::Endpoint {
                endpoint,
                vm: parse_vm_name(&req.parent).unwrap_or_default().into(),
            },
        })
    }
}

impl From<RegistryEntry> for QueryResult {
    fn from(val: RegistryEntry) -> Self {
        let status = if val.status.is_running() {
            VMStatus::Running
        } else if val.status.is_paused() {
            VMStatus::Paused
        } else {
            VMStatus::PoweredOff
        };
        let vm_name = val.vm_name().map(ToOwned::to_owned);
        let agent_name = val.agent_name().map(ToOwned::to_owned);
        QueryResult {
            name: val.name,
            description: val.status.description,
            status,
            trust_level: TrustLevel::default(),
            vm_type: val.r#type.vm,
            service_type: val.r#type.service,
            vm_name,
            agent_name,
        }
    }
}
