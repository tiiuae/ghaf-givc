// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, bail};
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

impl EndpointAddress {
    /// Build an endpoint address from transport protocol fields.
    ///
    /// # Errors
    /// Returns an error for unsupported protocols or invalid numeric ports/CIDs.
    pub fn from_protocol(
        protocol: impl AsRef<str>,
        address: impl AsRef<str>,
        port: impl AsRef<str>,
    ) -> anyhow::Result<Self> {
        let protocol = protocol.as_ref();
        let address = address.as_ref();
        let port = port.as_ref();

        match protocol {
            "tcp" => Ok(Self::Tcp {
                addr: address.to_owned(),
                port: port
                    .parse()
                    .with_context(|| format!("invalid tcp port {port}"))?,
            }),
            "unix" => Ok(Self::Unix(address.to_owned())),
            "abstract" => Ok(Self::Abstract(address.to_owned())),
            "vsock" => Ok(Self::Vsock(VsockAddr::new(
                address
                    .parse()
                    .with_context(|| format!("invalid vsock address (cid {address})"))?,
                port.parse()
                    .with_context(|| format!("invalid vsock address (port {port})"))?,
            ))),
            other => bail!("unsupported transport protocol: {other}"),
        }
    }
}

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

#[cfg(test)]
mod tests {
    use super::EndpointAddress;
    use tokio_listener::ListenerAddress;

    #[test]
    fn parses_tcp_transport_and_preserves_hostnames() {
        let endpoint = EndpointAddress::from_protocol("tcp", "192.0.2.20", "9014").unwrap();
        assert_eq!(
            endpoint,
            EndpointAddress::Tcp {
                addr: "192.0.2.20".to_owned(),
                port: 9014,
            }
        );
        let listener: ListenerAddress = endpoint.try_into().unwrap();
        assert!(matches!(
            listener,
            ListenerAddress::Tcp(address) if address == "192.0.2.20:9014".parse().unwrap()
        ));

        assert_eq!(
            EndpointAddress::from_protocol("tcp", "givc.example", "9014").unwrap(),
            EndpointAddress::Tcp {
                addr: "givc.example".to_owned(),
                port: 9014,
            }
        );
    }

    #[test]
    fn parses_unix_transport_without_port() {
        let endpoint = EndpointAddress::from_protocol("unix", "/run/givc/event.sock", "").unwrap();
        assert_eq!(
            endpoint,
            EndpointAddress::Unix("/run/givc/event.sock".to_owned())
        );
        let listener: ListenerAddress = endpoint.try_into().unwrap();
        assert!(matches!(
            listener,
            ListenerAddress::Path(path) if path == std::path::Path::new("/run/givc/event.sock")
        ));
    }

    #[test]
    fn parses_abstract_transport_without_port() {
        let endpoint = EndpointAddress::from_protocol("abstract", "givc-event", "").unwrap();
        assert_eq!(endpoint, EndpointAddress::Abstract("givc-event".to_owned()));
        let listener: ListenerAddress = endpoint.try_into().unwrap();
        assert!(matches!(
            listener,
            ListenerAddress::Abstract(name) if name == "givc-event"
        ));
    }

    #[test]
    fn parses_vsock_transport() {
        let endpoint = EndpointAddress::from_protocol("vsock", "3", "9014").unwrap();
        assert_eq!(
            endpoint,
            EndpointAddress::Vsock(tokio_vsock::VsockAddr::new(3, 9014))
        );
        let listener: ListenerAddress = endpoint.try_into().unwrap();
        assert!(matches!(
            listener,
            ListenerAddress::Vsock { cid: 3, port: 9014 }
        ));
    }

    #[test]
    fn rejects_invalid_tcp_port() {
        let error = EndpointAddress::from_protocol("tcp", "192.0.2.20", "not-a-port").unwrap_err();

        assert!(error.to_string().contains("invalid tcp port not-a-port"));
    }

    #[test]
    fn rejects_invalid_vsock_address_fields() {
        for (cid, port) in [("not-a-cid", "9014"), ("3", "not-a-port")] {
            let error = EndpointAddress::from_protocol("vsock", cid, port).unwrap_err();
            assert!(error.to_string().contains("invalid vsock address"));
        }
    }

    #[test]
    fn rejects_unsupported_protocol() {
        let error = EndpointAddress::from_protocol("udp", "192.0.2.20", "9014").unwrap_err();

        assert!(
            error
                .to_string()
                .contains("unsupported transport protocol: udp")
        );
    }

    #[test]
    fn listener_conversion_rejects_non_ip_tcp_address() {
        let endpoint = EndpointAddress::from_protocol("tcp", "not-an-ip", "9014").unwrap();
        let error =
            <EndpointAddress as TryInto<tokio_listener::ListenerAddress>>::try_into(endpoint)
                .unwrap_err();

        assert!(error.to_string().contains("not-an-ip"));
    }
}
