// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use givc_common::pb;
use givc_common::types::{EndpointEntry, UnitStatus, UnitType};
use tonic::Request;

pub(crate) fn registration_request(
    name: String,
    parent: String,
    ty: UnitType,
    endpoint: EndpointEntry,
    status: UnitStatus,
) -> pb::admin::RegistryRequest {
    pb::admin::RegistryRequest {
        name,
        parent,
        r#type: ty.into(),
        transport: Some(endpoint.into()),
        state: Some(status.into()),
    }
}

pub(crate) fn registration_rpc_request(
    name: String,
    parent: String,
    ty: UnitType,
    endpoint: EndpointEntry,
    status: UnitStatus,
) -> Request<pb::admin::RegistryRequest> {
    let mut request = Request::new(registration_request(name, parent, ty, endpoint, status));
    request.set_timeout(crate::endpoint::RPC_TIMEOUT);
    request
}

#[cfg(test)]
mod tests {
    use super::*;
    use givc_common::address::EndpointAddress;

    #[test]
    fn registration_request_preserves_parent_attribution() {
        let request = registration_request(
            "givc-agent.service".to_owned(),
            "app-vm".to_owned(),
            UnitType {
                vm: givc_common::types::VmType::AppVM,
                service: givc_common::types::ServiceType::Mgr,
            },
            EndpointEntry {
                address: EndpointAddress::Tcp {
                    addr: "127.0.0.1".to_owned(),
                    port: 9001,
                },
                tls_name: "app-vm".to_owned(),
            },
            UnitStatus {
                name: "givc-agent.service".to_owned(),
                description: "agent".to_owned(),
                load_state: "loaded".to_owned(),
                active_state: "active".to_owned(),
                sub_state: "running".to_owned(),
                path: "/org/freedesktop/systemd1/unit/givc_agent_2eservice".to_owned(),
                freezer_state: "running".to_owned(),
            },
        );

        assert_eq!(request.parent, "app-vm");
    }

    #[test]
    fn service_registration_request_preserves_agent_parent() {
        let request = registration_request(
            "dbus.service".to_owned(),
            "givc-appvm.service".to_owned(),
            UnitType {
                vm: givc_common::types::VmType::AppVM,
                service: givc_common::types::ServiceType::Svc,
            },
            EndpointEntry {
                address: EndpointAddress::Tcp {
                    addr: "127.0.0.1".to_owned(),
                    port: 9001,
                },
                tls_name: "app-vm".to_owned(),
            },
            UnitStatus {
                name: "dbus.service".to_owned(),
                description: "dbus".to_owned(),
                load_state: "loaded".to_owned(),
                active_state: "active".to_owned(),
                sub_state: "running".to_owned(),
                path: "/org/freedesktop/systemd1/unit/dbus_2eservice".to_owned(),
                freezer_state: "running".to_owned(),
            },
        );

        assert_eq!(request.parent, "givc-appvm.service");
    }

    #[test]
    fn registration_rpc_request_preserves_parent_and_deadline() {
        let request = registration_rpc_request(
            "givc-agent.service".to_owned(),
            "app-vm".to_owned(),
            UnitType {
                vm: givc_common::types::VmType::AppVM,
                service: givc_common::types::ServiceType::Mgr,
            },
            EndpointEntry {
                address: EndpointAddress::Tcp {
                    addr: "127.0.0.1".to_owned(),
                    port: 9001,
                },
                tls_name: "app-vm".to_owned(),
            },
            UnitStatus {
                name: "givc-agent.service".to_owned(),
                description: "agent".to_owned(),
                load_state: "loaded".to_owned(),
                active_state: "active".to_owned(),
                sub_state: "running".to_owned(),
                path: "/org/freedesktop/systemd1/unit/givc_agent_2eservice".to_owned(),
                freezer_state: "running".to_owned(),
            },
        );

        assert_eq!(request.get_ref().parent, "app-vm");
        assert_eq!(request.metadata().get("grpc-timeout").unwrap(), "5000000u");
    }
}
