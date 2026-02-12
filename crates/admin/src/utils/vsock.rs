// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use anyhow::bail;
use tokio_vsock::{VMADDR_CID_HOST, VMADDR_CID_LOCAL, VsockAddr};

/// # Errors
/// Return `Err` if vsock address is invalid
pub fn parse_vsock_addr(addr: &str) -> anyhow::Result<VsockAddr> {
    if let Some((cid, port)) = addr.split_once(':') {
        let cid = match cid {
            "local" => VMADDR_CID_LOCAL,
            "host" => VMADDR_CID_HOST,
            cid => cid.parse()?,
        };
        return Ok(VsockAddr::new(cid, port.parse()?));
    }
    bail!("Address {addr} should be in CID:PORT format")
}
