// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use givc_common::pb;
use tonic::{Request, Response, Status};

pub use pb::hwid::hwid_service_server::HwidServiceServer;

#[derive(Debug, Clone)]
pub struct HwIdServer {
    iface: String,
}

impl HwIdServer {
    #[must_use]
    pub fn new(iface: String) -> Result<Self> {
        Ok(Self {
            iface: select_interface(iface)?,
        })
    }

    /// # Errors
    /// Fails if the interface does not exist, is down, or cannot read its MAC address.
    pub fn get_identifier(&self) -> Result<String> {
        if self.iface.is_empty() {
            bail!("could not find wireless or ethernet device")
        }

        let operstate = read_trimmed(format!("/sys/class/net/{}/operstate", self.iface))?;
        if operstate != "up" {
            bail!("interface is down, could report unreliable information")
        }

        let hwaddr = read_trimmed(format!("/sys/class/net/{}/address", self.iface))?;
        Ok(hwaddr)
    }
}

#[tonic::async_trait]
impl pb::hwid::hwid_service_server::HwidService for HwIdServer {
    async fn get_hw_id(
        &self,
        _request: Request<pb::hwid::HwIdRequest>,
    ) -> Result<Response<pb::hwid::HwIdResponse>, Status> {
        let identifier = self.get_identifier().map_err(map_err)?;
        Ok(Response::new(pb::hwid::HwIdResponse { identifier }))
    }
}

fn select_interface(iface: String) -> Result<String> {
    select_interface_from("/sys/class/net", iface)
}

fn select_interface_from(net_path: &str, iface: String) -> Result<String> {
    if !iface.is_empty() {
        return Ok(iface);
    }

    for prefix in ["wl", "en"] {
        if let Some(found) = find_interface_at(net_path, prefix)? {
            return Ok(found);
        }
    }

    bail!("could not find wireless or ethernet device")
}

fn find_interface_at(net_path: &str, prefix: &str) -> Result<Option<String>> {
    let entries = fs::read_dir(net_path).context("error querying network device name")?;
    for entry in entries {
        let entry = entry.context("error querying network device name")?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(prefix) {
            return Ok(Some(name));
        }
    }
    Ok(None)
}

fn read_trimmed(path: impl AsRef<Path>) -> Result<String> {
    Ok(fs::read_to_string(path)?.trim().to_owned())
}

fn map_err(err: anyhow::Error) -> Status {
    // Add hardware-ID context while keeping the error internal.
    Status::internal(format!("cannot get hardware id: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefer_explicit_iface() {
        assert_eq!(select_interface("eth0".to_owned()).unwrap(), "eth0");
    }

    #[test]
    fn absent_nic_returns_descriptive_error() {
        let empty_net = std::env::temp_dir().join(format!("givc-no-nic-{}", std::process::id()));
        let _ = fs::remove_dir_all(&empty_net);
        fs::create_dir(&empty_net).unwrap();
        let error = select_interface_from(empty_net.to_str().unwrap(), String::new())
            .unwrap_err()
            .to_string();
        fs::remove_dir_all(empty_net).unwrap();
        assert!(error.contains("could not find wireless or ethernet device"));
    }
}
