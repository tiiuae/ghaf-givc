// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use givc_common::pb;
use serde_json::Value as JsonValue;
use tokio::time::sleep;
use tonic::{Request, Response, Status};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};
use zbus::{Connection, Proxy};

pub use pb::wifi::wifi_service_server::WifiServiceServer as WifiServiceServerServer;

const NM_SERVICE: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";
const IFACE_NM: &str = "org.freedesktop.NetworkManager";
const IFACE_DEVICE: &str = "org.freedesktop.NetworkManager.Device";
const IFACE_DEVICE_WIRELESS: &str = "org.freedesktop.NetworkManager.Device.Wireless";
const IFACE_ACCESS_POINT: &str = "org.freedesktop.NetworkManager.AccessPoint";
const IFACE_CONNECTION_ACTIVE: &str = "org.freedesktop.NetworkManager.Connection.Active";

const NM_DEVICE_TYPE_WIFI: u32 = 2;
const NM_ACTIVE_CONNECTION_STATE_UNKNOWN: u32 = 0;
const NM_ACTIVE_CONNECTION_STATE_ACTIVATING: u32 = 1;
const NM_ACTIVE_CONNECTION_STATE_ACTIVATED: u32 = 2;
const NM_ACTIVE_CONNECTION_STATE_DEACTIVATING: u32 = 3;
const NM_ACTIVE_CONNECTION_STATE_DEACTIVATED: u32 = 4;

const NM_AP_SEC_NONE: u32 = 0x0;
const NM_AP_SEC_KEY_MGMT_8021X: u32 = 0x200;
const NM_AP_SEC_KEY_MGMT_OWE: u32 = 0x800;
const NM_AP_SEC_KEY_MGMT_OWE_TM: u32 = 0x1000;
const NM_AP_SEC_KEY_MGMT_PSK: u32 = 0x100;
const NM_AP_SEC_KEY_MGMT_SAE: u32 = 0x400;

type Settings = HashMap<String, HashMap<String, OwnedValue>>;

#[derive(Debug, Clone)]
pub struct WifiService {
    backend: Arc<NetworkManagerBackend>,
}

#[derive(Debug)]
struct NetworkManagerBackend {
    conn: Connection,
}

#[derive(Debug, Clone)]
struct AccessPointInfo {
    connection: bool,
    ssid: String,
    signal: u32,
    security: String,
}

impl NetworkManagerBackend {
    async fn new() -> Result<Self> {
        Ok(Self {
            conn: Connection::system()
                .await
                .context("failed to connect to system bus")?,
        })
    }

    async fn nm_proxy(&self) -> Result<Proxy<'_>> {
        Ok(Proxy::new(&self.conn, NM_SERVICE, NM_PATH, IFACE_NM).await?)
    }

    async fn proxy<'a>(&'a self, path: &'a OwnedObjectPath, iface: &'a str) -> Result<Proxy<'a>> {
        Ok(Proxy::new(&self.conn, NM_SERVICE, path.as_str(), iface).await?)
    }

    async fn device_paths(&self) -> Result<Vec<OwnedObjectPath>> {
        let nm = self.nm_proxy().await?;
        let device_paths: Vec<OwnedObjectPath> = nm.call("GetDevices", &()).await?;
        Ok(device_paths)
    }

    async fn wifi_devices(&self) -> Result<Vec<OwnedObjectPath>> {
        let mut wifi_devices = Vec::new();
        for device_path in self.device_paths().await? {
            let device = self.proxy(&device_path, IFACE_DEVICE).await?;
            let managed: bool = device.get_property("Managed").await?;
            if !managed {
                continue;
            }
            let device_type: u32 = device.get_property("DeviceType").await?;
            if device_type == NM_DEVICE_TYPE_WIFI {
                wifi_devices.push(device_path);
            }
        }
        Ok(wifi_devices)
    }

    async fn access_points(&self, device_path: &OwnedObjectPath) -> Result<Vec<OwnedObjectPath>> {
        let device = self.proxy(device_path, IFACE_DEVICE_WIRELESS).await?;
        let ap_paths: Vec<OwnedObjectPath> = device.call("GetAllAccessPoints", &()).await?;
        Ok(ap_paths)
    }

    async fn ap_info(&self, ap_path: &OwnedObjectPath) -> Result<AccessPointInfo> {
        let ap = self.proxy(ap_path, IFACE_ACCESS_POINT).await?;
        let ssid: Vec<u8> = ap.get_property("Ssid").await?;
        let signal: u8 = ap.get_property("Strength").await?;
        let flags: u32 = ap.get_property("Flags").await?;
        let wpa_flags: u32 = ap.get_property("WpaFlags").await?;
        let rsn_flags: u32 = ap.get_property("RsnFlags").await?;

        Ok(AccessPointInfo {
            connection: false,
            ssid: String::from_utf8_lossy(&ssid).to_string(),
            signal: u32::from(signal),
            security: infer_security(flags, wpa_flags, rsn_flags),
        })
    }

    async fn active_connection(&self) -> Result<Option<AccessPointInfo>> {
        for device_path in self.wifi_devices().await? {
            let device = self.proxy(&device_path, IFACE_DEVICE_WIRELESS).await?;
            let ap_path: OwnedObjectPath = device.get_property("ActiveAccessPoint").await?;
            if ap_path.as_str() == "/" {
                continue;
            }

            let mut ap = self.ap_info(&ap_path).await?;
            ap.connection = true;
            return Ok(Some(ap));
        }

        Ok(None)
    }

    async fn connect(&self, ssid: &str, password: &str, settings_ext: &str) -> Result<String> {
        for device_path in self.wifi_devices().await? {
            let ap_paths = self.access_points(&device_path).await?;
            for ap_path in ap_paths {
                let ap = self.ap_info(&ap_path).await?;
                if ap.ssid != ssid {
                    continue;
                }

                let keymgmt = determine_key_mgmt(&ap);
                let mut settings = build_connection_settings(ssid, password, &keymgmt)?;
                if keymgmt == "wpa-eap" {
                    settings = merge_settings(settings, settings_ext)?;
                }

                let nm = self.nm_proxy().await?;
                let root = OwnedObjectPath::try_from("/").context("invalid root path")?;
                let (_connection_path, active_connection_path): (OwnedObjectPath, OwnedObjectPath) =
                    nm.call(
                        "AddAndActivateConnection",
                        &(settings, device_path.clone(), root),
                    )
                    .await?;

                self.wait_for_activation(&active_connection_path).await?;
                return Ok(format!("Connected to {ssid} successfully"));
            }
        }

        bail!("failed to add or connect to {ssid}")
    }

    async fn wait_for_activation(&self, active_path: &OwnedObjectPath) -> Result<()> {
        let active = self.proxy(active_path, IFACE_CONNECTION_ACTIVE).await?;
        loop {
            let state: u32 = active.get_property("State").await?;
            match state {
                NM_ACTIVE_CONNECTION_STATE_UNKNOWN => {
                    bail!("unexpected active connection state: unknown")
                }
                NM_ACTIVE_CONNECTION_STATE_ACTIVATING => {}
                NM_ACTIVE_CONNECTION_STATE_ACTIVATED => return Ok(()),
                NM_ACTIVE_CONNECTION_STATE_DEACTIVATING => {
                    bail!("unexpected active connection state: deactivating")
                }
                NM_ACTIVE_CONNECTION_STATE_DEACTIVATED => {
                    bail!("unexpected active connection state: deactivated")
                }
                _ => bail!("unknown active connection state"),
            }
            sleep(Duration::from_millis(100)).await;
        }
    }

    async fn disconnect(&self) -> Result<String> {
        let mut response = "wifi disconnection is failed".to_owned();
        for device_path in self.wifi_devices().await? {
            let device = self.proxy(&device_path, IFACE_DEVICE).await?;
            let _: () = device.call("Disconnect", &()).await?;
            let intf: String = device.get_property("Interface").await?;
            response = format!("{intf} disconnected successfully");
        }
        Ok(response)
    }

    async fn radio_switch(&self, turn_on: bool) -> Result<String> {
        let nm = self.nm_proxy().await?;
        nm.set_property("WirelessEnabled", turn_on).await?;
        let _: bool = nm.get_property("WirelessEnabled").await?;
        let status = if turn_on { "enabled" } else { "disabled" };
        Ok(format!("Wireless {status} successfully"))
    }
}

impl WifiService {
    pub async fn new() -> Result<Self> {
        Ok(Self {
            backend: Arc::new(NetworkManagerBackend::new().await?),
        })
    }
}

#[tonic::async_trait]
impl pb::wifi::wifi_service_server::WifiService for WifiService {
    async fn list_network(
        &self,
        _request: Request<pb::wifi::WifiNetworkRequest>,
    ) -> Result<Response<pb::wifi::WifiNetworkResponse>, Status> {
        let mut networks = Vec::new();
        for device_path in self.backend.wifi_devices().await.map_err(map_err)? {
            for ap_path in self
                .backend
                .access_points(&device_path)
                .await
                .map_err(map_err)?
            {
                let ap = self.backend.ap_info(&ap_path).await.map_err(map_err)?;
                networks.push(ap.into());
            }
        }

        Ok(Response::new(pb::wifi::WifiNetworkResponse { networks }))
    }

    async fn get_active_connection(
        &self,
        _request: Request<pb::wifi::EmptyRequest>,
    ) -> Result<Response<pb::wifi::AccessPoint>, Status> {
        if let Some(ap) = self.backend.active_connection().await.map_err(map_err)? {
            return Ok(Response::new(ap.into()));
        }

        Ok(Response::new(pb::wifi::AccessPoint {
            connection: false,
            ssid: String::new(),
            signal: 0,
            security: String::new(),
        }))
    }

    async fn connect_network(
        &self,
        request: Request<pb::wifi::WifiConnectionRequest>,
    ) -> Result<Response<pb::wifi::WifiConnectionResponse>, Status> {
        let req = request.into_inner();
        let response = self
            .backend
            .connect(&req.ssid, &req.password, &req.settings)
            .await
            .map_err(map_err)?;
        Ok(Response::new(pb::wifi::WifiConnectionResponse { response }))
    }

    async fn disconnect_network(
        &self,
        _request: Request<pb::wifi::EmptyRequest>,
    ) -> Result<Response<pb::wifi::WifiConnectionResponse>, Status> {
        let response = self.backend.disconnect().await.map_err(map_err)?;
        Ok(Response::new(pb::wifi::WifiConnectionResponse { response }))
    }

    async fn turn_on(
        &self,
        _request: Request<pb::wifi::EmptyRequest>,
    ) -> Result<Response<pb::wifi::WifiConnectionResponse>, Status> {
        let response = self.backend.radio_switch(true).await.map_err(map_err)?;
        Ok(Response::new(pb::wifi::WifiConnectionResponse { response }))
    }

    async fn turn_off(
        &self,
        _request: Request<pb::wifi::EmptyRequest>,
    ) -> Result<Response<pb::wifi::WifiConnectionResponse>, Status> {
        let response = self.backend.radio_switch(false).await.map_err(map_err)?;
        Ok(Response::new(pb::wifi::WifiConnectionResponse { response }))
    }
}

impl From<AccessPointInfo> for pb::wifi::AccessPoint {
    fn from(value: AccessPointInfo) -> Self {
        Self {
            connection: value.connection,
            ssid: value.ssid,
            signal: value.signal,
            security: value.security,
        }
    }
}

fn map_err(err: anyhow::Error) -> Status {
    Status::internal(err.to_string())
}

fn infer_security(flags: u32, wpa_flags: u32, rsn_flags: u32) -> String {
    let mut security = String::new();
    if flags != NM_AP_SEC_NONE && wpa_flags == NM_AP_SEC_NONE && rsn_flags == NM_AP_SEC_NONE {
        security.push_str("WEP ");
    }
    if wpa_flags != NM_AP_SEC_NONE {
        security.push_str("WPA ");
    }
    if rsn_flags & NM_AP_SEC_KEY_MGMT_PSK != 0 || rsn_flags & NM_AP_SEC_KEY_MGMT_8021X != 0 {
        security.push_str("WPA2 ");
    }
    if rsn_flags & NM_AP_SEC_KEY_MGMT_SAE != 0 {
        security.push_str("WPA3 ");
    }
    if rsn_flags & NM_AP_SEC_KEY_MGMT_OWE != 0 || rsn_flags & NM_AP_SEC_KEY_MGMT_OWE_TM != 0 {
        security.push_str("OWE ");
    }
    if wpa_flags & NM_AP_SEC_KEY_MGMT_8021X != 0 || rsn_flags & NM_AP_SEC_KEY_MGMT_8021X != 0 {
        security.push_str("802.1X ");
    }
    if flags != NM_AP_SEC_NONE && wpa_flags != NM_AP_SEC_NONE && rsn_flags != NM_AP_SEC_NONE {
        security = "None".to_owned();
    }
    security
}

fn determine_key_mgmt(ap: &AccessPointInfo) -> String {
    if ap.security.contains("802.1X") {
        "wpa-eap".to_owned()
    } else if ap.security.contains("OWE") {
        "owe".to_owned()
    } else if ap.security.contains("WPA3") {
        "sae".to_owned()
    } else if ap.security.contains("WPA2") {
        "wpa-psk".to_owned()
    } else if ap.security.contains("WEP") {
        "ieee8021x".to_owned()
    } else {
        "none".to_owned()
    }
}

fn build_connection_settings(ssid: &str, password: &str, keymgmt: &str) -> Result<Settings> {
    let mut settings = Settings::new();
    settings.insert(
        "connection".to_owned(),
        hashmap_values([
            ("id", owned(ssid.to_owned())?),
            ("type", owned("802-11-wireless")?),
            ("autoconnect", owned(true)?),
        ]),
    );
    settings.insert(
        "802-11-wireless".to_owned(),
        hashmap_values([
            ("ssid", owned(ssid.as_bytes().to_vec())?),
            ("mode", owned("infrastructure")?),
            ("security", owned("802-11-wireless-security")?),
        ]),
    );
    settings.insert(
        "802-11-wireless-security".to_owned(),
        hashmap_values([
            ("key-mgmt", owned(keymgmt.to_owned())?),
            ("psk", owned(password.to_owned())?),
        ]),
    );
    Ok(settings)
}

fn merge_settings(mut base: Settings, raw: &str) -> Result<Settings> {
    let parsed: JsonValue =
        serde_json::from_str(raw).context("failed to parse extension settings")?;
    let JsonValue::Object(sections) = parsed else {
        bail!("extension settings must be a JSON object");
    };

    for (section, values) in sections {
        let JsonValue::Object(keys) = values else {
            bail!("extension section '{section}' must be a JSON object");
        };

        let entry = base.entry(section).or_default();
        for (key, value) in keys {
            entry.insert(key, json_value_to_owned(value)?);
        }
    }

    Ok(base)
}

fn json_value_to_owned(value: JsonValue) -> Result<OwnedValue> {
    match value {
        JsonValue::Null => bail!("null is not supported in wifi settings"),
        JsonValue::Bool(v) => Ok(OwnedValue::try_from(Value::new(v))?),
        JsonValue::Number(v) if v.is_i64() => {
            Ok(OwnedValue::try_from(Value::new(v.as_i64().unwrap()))?)
        }
        JsonValue::Number(v) if v.is_u64() => {
            Ok(OwnedValue::try_from(Value::new(v.as_u64().unwrap()))?)
        }
        JsonValue::Number(v) => Ok(OwnedValue::try_from(Value::new(v.as_f64().unwrap()))?),
        JsonValue::String(v) => Ok(OwnedValue::try_from(Value::new(v))?),
        JsonValue::Array(v) => {
            let mut items = Vec::with_capacity(v.len());
            for item in v {
                items.push(json_value_to_owned(item)?);
            }
            Ok(OwnedValue::try_from(Value::new(items))?)
        }
        JsonValue::Object(v) => {
            let mut map = HashMap::new();
            for (key, value) in v {
                map.insert(key, json_value_to_owned(value)?);
            }
            Ok(OwnedValue::try_from(Value::new(map))?)
        }
    }
}

fn hashmap_values<const N: usize>(entries: [(&str, OwnedValue); N]) -> HashMap<String, OwnedValue> {
    let mut map = HashMap::with_capacity(N);
    for (key, value) in entries {
        map.insert(key.to_owned(), value);
    }
    map
}

fn owned<T>(value: T) -> Result<OwnedValue>
where
    T: Into<Value<'static>> + zbus::zvariant::DynamicType,
{
    Ok(OwnedValue::try_from(Value::new(value))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn security_tree_prefers_flags() {
        let ap = AccessPointInfo {
            connection: false,
            ssid: "ssid".to_owned(),
            signal: 1,
            security: "WPA2 WPA3 802.1X".to_owned(),
        };
        assert_eq!(determine_key_mgmt(&ap), "wpa-eap");
    }

    #[test]
    fn merges_extension_settings() {
        let base = build_connection_settings("ssid", "pass", "wpa-psk").unwrap();
        let merged =
            merge_settings(base, r#"{"802-11-wireless-security":{"proto":"rsn"}}"#).unwrap();
        assert!(merged["802-11-wireless-security"].contains_key("proto"));
    }
}
