use anyhow::bail;
use tokio_vsock::{VsockAddr, VMADDR_CID_HOST, VMADDR_CID_LOCAL};

pub fn parse_vsock_addr(addr: &str) -> anyhow::Result<VsockAddr> {
    if let Some((cid, port)) = addr.split_once(':') {
        let cid = match cid {
            "local" => VMADDR_CID_LOCAL,
            "host" => VMADDR_CID_HOST,
            cid => cid.parse()?,
        };
        return Ok(VsockAddr::new(cid, port.parse()?));
    };
    bail!("Address {addr} should be in CID:PORT format")
}
