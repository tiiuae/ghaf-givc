// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use anyhow::Context;
use std::net::{IpAddr, SocketAddr};
use tokio_listener::ListenerAddress;
use tokio_vsock::VsockAddr;

#[derive(Clone, Debug, PartialEq)]
pub enum EndpointAddress {
    Tcp {
        // IP + port (FIXME: should be SocketAddres)
        addr: String,
        port: u16,
    },
    Unix(String),     // "/path/to/sock"  (same host only)
    Abstract(String), // "@abstract-socket-name" (same host only)
    Vsock(VsockAddr), // cid+port. FIXME: cid have two magic numbers for host and local
}

//impl EndpointAddress {
//    pub fn from_protocol(protocol: impl AsRef<str>, addres: impl AsRef<str>, port: impl AsRef<str>) -> anyhow::Result<Self> {
//    }
//}

impl TryInto<ListenerAddress> for EndpointAddress {
    type Error = anyhow::Error;
    fn try_into(self) -> anyhow::Result<ListenerAddress> {
        match self {
            EndpointAddress::Tcp { addr, port } => {
                let ip_addr: IpAddr = addr
                    .parse()
                    .with_context(|| format!("while parsing TCP address {addr}"))?;
                Ok(ListenerAddress::Tcp(SocketAddr::new(ip_addr, port)))
            }
            EndpointAddress::Unix(path) => Ok(ListenerAddress::Path(path.into())),
            EndpointAddress::Abstract(name) => Ok(ListenerAddress::Abstract(name)),
            EndpointAddress::Vsock(vsock) => Ok(ListenerAddress::Vsock {
                cid: vsock.cid(),
                port: vsock.port(),
            }),
        }
    }
}
