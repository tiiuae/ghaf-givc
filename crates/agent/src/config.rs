// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::{collections::HashMap, net::SocketAddr, path::Path, path::PathBuf};

use anyhow::{Context, Result, bail};
use givc_client::endpoint::TlsConfig;
use givc_common::types::{ServiceType, UnitType, VmType};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct AgentConfig {
    #[serde(default)]
    pub identity: IdentityConfig,

    #[serde(default)]
    pub network: NetworkConfig,

    #[serde(default)]
    pub capabilities: CapabilitiesConfig,

    #[serde(rename = "accessControl", default)]
    pub access_control: AccessControlConfig,
}

impl AgentConfig {
    /// # Errors
    /// Fails if file read or JSON parse fails.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let data = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("failed to read config file {}", path.as_ref().display()))?;
        let mut config: Self = serde_json::from_str(&data)
            .with_context(|| format!("failed to parse JSON config {}", path.as_ref().display()))?;
        config.populate()?;
        Ok(config)
    }

    /// # Errors
    /// Fails if endpoint transport cannot be used for this runtime.
    pub fn listen_addr(&self) -> Result<SocketAddr> {
        self.network.agent.transport.listen_addr()
    }

    /// # Errors
    /// Fails if TLS config cannot be derived.
    fn populate(&mut self) -> Result<()> {
        self.identity.service_name = format!("givc-{}.service", self.identity.name);

        self.capabilities.units = HashMap::new();
        for service in &self.capabilities.services {
            self.capabilities
                .units
                .insert(service.clone(), self.identity.sub_type);
        }

        if !self.capabilities.vm_services.admin_vm.is_empty() {
            self.capabilities.units.insert(
                self.capabilities.vm_services.admin_vm.clone(),
                u32::from(UnitType {
                    vm: VmType::AdmVM,
                    service: ServiceType::VM,
                }),
            );
        }

        for vm in &self.capabilities.vm_services.sys_vms {
            self.capabilities.units.insert(
                vm.clone(),
                u32::from(UnitType {
                    vm: VmType::SysVM,
                    service: ServiceType::VM,
                }),
            );
        }

        for vm in &self.capabilities.vm_services.app_vms {
            self.capabilities.units.insert(
                vm.clone(),
                u32::from(UnitType {
                    vm: VmType::AppVM,
                    service: ServiceType::VM,
                }),
            );
        }

        self.network.tls_config = if self.network.tls.enable {
            Some(self.network.tls.to_tls_config()?)
        } else {
            None
        };

        self.network.admin.services.clear();
        self.network.admin.tls_config = self.network.tls_config.clone();

        self.network.agent.services = std::iter::once(self.identity.service_name.clone())
            .chain(self.capabilities.units.keys().cloned())
            .collect();
        self.network.agent.tls_config = self.network.tls_config.clone();
        self.network.agent.acl_config = self.access_control.clone();

        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct IdentityConfig {
    #[serde(rename = "type", default)]
    pub r#type: u32,

    #[serde(rename = "subType", default)]
    pub sub_type: u32,

    #[serde(default)]
    pub parent: String,

    #[serde(default)]
    pub name: String,

    #[serde(skip, default)]
    pub service_name: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct NetworkConfig {
    #[serde(rename = "admin", default)]
    pub admin: EndpointConfig,

    #[serde(rename = "agent", default)]
    pub agent: EndpointConfig,

    #[serde(default)]
    pub tls: TlsConfigJson,

    #[serde(skip, default)]
    pub tls_config: Option<TlsConfig>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct EndpointConfig {
    #[serde(default)]
    pub transport: TransportConfig,

    #[serde(skip, default)]
    pub services: Vec<String>,

    #[serde(skip, default)]
    pub tls_config: Option<TlsConfig>,

    #[serde(skip, default)]
    pub acl_config: AccessControlConfig,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TransportConfig {
    #[serde(default)]
    pub protocol: String,

    #[serde(rename = "addr", default)]
    pub address: String,

    #[serde(default)]
    pub port: String,

    #[serde(default)]
    pub name: String,
}

impl TransportConfig {
    /// # Errors
    /// Fails if protocol is unsupported or address is invalid.
    pub fn listen_addr(&self) -> Result<SocketAddr> {
        match self.protocol.as_str() {
            "tcp" => {
                let addr: std::net::IpAddr = self
                    .address
                    .parse()
                    .with_context(|| format!("invalid tcp address {}", self.address))?;
                let port: u16 = self
                    .port
                    .parse()
                    .with_context(|| format!("invalid tcp port {}", self.port))?;
                Ok(SocketAddr::new(addr, port))
            }
            other => bail!("unsupported listen protocol: {other}"),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TlsConfigJson {
    #[serde(default)]
    pub enable: bool,

    #[serde(rename = "caCertPath", default)]
    pub ca_cert_path: PathBuf,

    #[serde(rename = "certPath", default)]
    pub cert_path: PathBuf,

    #[serde(rename = "keyPath", default)]
    pub key_path: PathBuf,
}

impl TlsConfigJson {
    /// # Errors
    /// Fails if TLS files cannot be read later by tonic.
    pub fn to_tls_config(&self) -> Result<TlsConfig> {
        Ok(TlsConfig {
            ca_cert_file_path: self.ca_cert_path.clone(),
            cert_file_path: self.cert_path.clone(),
            key_file_path: self.key_path.clone(),
            tls_name: None,
        })
    }
}

fn null_to_empty_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    Option::<Vec<T>>::deserialize(deserializer).map(|value| value.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_null_collection_fields_as_empty() {
        let json = r#"{
            "identity": {"name": "vm", "type": 1, "subType": 2, "parent": "parent"},
            "network": {
                "admin": {"transport": {"addr": "127.0.0.1", "port": "9000", "protocol": "tcp"}},
                "agent": {"transport": {"addr": "127.0.0.1", "port": "9001", "protocol": "tcp"}},
                "tls": {"enable": false}
            },
            "capabilities": {
                "services": [],
                "applications": [],
                "exec": {"enable": false},
                "wifi": {"enable": false},
                "ctap": {"enable": false},
                "hwid": {"enable": false, "interface": ""},
                "notifier": {"enable": false, "socket": ""},
                "eventProxy": {"enable": false, "events": null},
                "socketProxy": {"enable": false, "sockets": null},
                "policy": {"enable": false, "storePath": "", "policies": {}}
            }
        }"#;

        let config: AgentConfig = serde_json::from_str(json).expect("config should parse");
        assert!(config.capabilities.event_proxy.events.is_empty());
        assert!(config.capabilities.socket_proxy.sockets.is_empty());
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct AccessControlConfig {
    #[serde(rename = "enable", default)]
    pub enabled: bool,

    #[serde(rename = "rulesFile", default)]
    pub rules_file: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct CapabilitiesConfig {
    #[serde(default)]
    #[serde(deserialize_with = "null_to_empty_vec")]
    pub services: Vec<String>,

    #[serde(rename = "vmServices", default)]
    pub vm_services: VmServicesConfig,

    #[serde(default)]
    #[serde(deserialize_with = "null_to_empty_vec")]
    pub applications: Vec<ApplicationManifest>,

    #[serde(default)]
    pub exec: ToggleConfig,

    #[serde(default)]
    pub wifi: ToggleConfig,

    #[serde(default)]
    pub ctap: ToggleConfig,

    #[serde(default)]
    pub hwid: HwidConfig,

    #[serde(default)]
    pub notifier: NotifierConfig,

    #[serde(rename = "eventProxy", default)]
    pub event_proxy: EventProxyConfig,

    #[serde(rename = "socketProxy", default)]
    pub socket_proxy: SocketProxyConfig,

    #[serde(default)]
    pub policy: PolicyConfig,

    #[serde(skip, default)]
    pub units: HashMap<String, u32>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct VmServicesConfig {
    #[serde(rename = "adminVm", default)]
    pub admin_vm: String,

    #[serde(rename = "systemVms", default)]
    #[serde(deserialize_with = "null_to_empty_vec")]
    pub sys_vms: Vec<String>,

    #[serde(rename = "appVms", default)]
    #[serde(deserialize_with = "null_to_empty_vec")]
    pub app_vms: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ApplicationManifest {
    #[serde(default)]
    pub name: String,

    #[serde(default)]
    pub command: String,

    #[serde(default)]
    #[serde(deserialize_with = "null_to_empty_vec")]
    pub args: Vec<String>,

    #[serde(default)]
    #[serde(deserialize_with = "null_to_empty_vec")]
    pub directories: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ToggleConfig {
    #[serde(rename = "enable", default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct HwidConfig {
    #[serde(rename = "enable", default)]
    pub enabled: bool,

    #[serde(default)]
    pub interface: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct NotifierConfig {
    #[serde(rename = "enable", default)]
    pub enabled: bool,

    #[serde(default)]
    pub socket: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct EventProxyConfig {
    #[serde(rename = "enable", default)]
    pub enabled: bool,

    #[serde(default)]
    #[serde(deserialize_with = "null_to_empty_vec")]
    pub events: Vec<EventConfig>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct EventConfig {
    #[serde(default)]
    pub transport: TransportConfig,

    #[serde(rename = "producer", default)]
    pub producer: bool,

    #[serde(default)]
    pub device: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SocketProxyConfig {
    #[serde(rename = "enable", default)]
    pub enabled: bool,

    #[serde(default)]
    #[serde(deserialize_with = "null_to_empty_vec")]
    pub sockets: Vec<ProxyConfig>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ProxyConfig {
    #[serde(default)]
    pub transport: TransportConfig,

    #[serde(rename = "server", default)]
    pub server: bool,

    #[serde(default)]
    pub socket: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct PolicyConfig {
    #[serde(rename = "enable", default)]
    pub enabled: bool,

    #[serde(rename = "storePath", default)]
    pub store_path: String,

    #[serde(default)]
    pub policies: HashMap<String, String>,
}
