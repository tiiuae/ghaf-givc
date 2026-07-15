// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::ffi::CString;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::mem::{size_of, zeroed};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use givc_client::endpoint::EndpointConfig as ClientEndpointConfig;
use givc_common::address::EndpointAddress;
use givc_common::pb;
use givc_common::types::TransportConfig as CommonTransportConfig;
use tokio::runtime::Handle;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::{error, info, warn};

use crate::config::{AgentConfig, EventConfig};

pub use pb::eventproxy::event_service_server::EventServiceServer as EventProxyServerServer;

const UINPUT_NAME_MOUSE: &str = "givc-virtual-mouse";
const UINPUT_NAME_GAMEPAD: &str = "givc-virtual-gamepad";
const BUS_USB: u16 = 0x03;

const EV_SYN: u16 = 0x00;
const EV_KEY: u16 = 0x01;
const EV_REL: u16 = 0x02;
const EV_ABS: u16 = 0x03;

const SYN_REPORT: u16 = 0x00;

const REL_X: u16 = 0x00;
const REL_Y: u16 = 0x01;
const REL_WHEEL: u16 = 0x08;

const ABS_X: u16 = 0x00;
const ABS_Y: u16 = 0x01;
const ABS_Z: u16 = 0x02;
const ABS_RX: u16 = 0x03;
const ABS_RY: u16 = 0x04;
const ABS_RZ: u16 = 0x05;
const ABS_HAT0X: u16 = 0x10;
const ABS_HAT0Y: u16 = 0x11;

const BTN_LEFT: u16 = 0x110;
const BTN_RIGHT: u16 = 0x111;
const BTN_MIDDLE: u16 = 0x112;

const BTN_SOUTH: u16 = 0x130;
const BTN_EAST: u16 = 0x131;
const BTN_C: u16 = 0x132;
const BTN_NORTH: u16 = 0x133;
const BTN_WEST: u16 = 0x134;
const BTN_TL: u16 = 0x136;
const BTN_TR: u16 = 0x137;
const BTN_SELECT: u16 = 0x13a;
const BTN_START: u16 = 0x13b;
const BTN_MODE: u16 = 0x13c;
const BTN_THUMBL: u16 = 0x13d;
const BTN_THUMBR: u16 = 0x13e;
const BTN_DPAD_UP: u16 = 0x220;
const BTN_DPAD_DOWN: u16 = 0x221;
const BTN_DPAD_LEFT: u16 = 0x222;
const BTN_DPAD_RIGHT: u16 = 0x223;

const IOC_NRBITS: u32 = 8;
const IOC_TYPEBITS: u32 = 8;
const IOC_SIZEBITS: u32 = 14;

const IOC_NRSHIFT: u32 = 0;
const IOC_TYPESHIFT: u32 = IOC_NRSHIFT + IOC_NRBITS;
const IOC_SIZESHIFT: u32 = IOC_TYPESHIFT + IOC_TYPEBITS;
const IOC_DIRSHIFT: u32 = IOC_SIZESHIFT + IOC_SIZEBITS;

const IOC_NONE: u32 = 0;
const IOC_WRITE: u32 = 1;
const IOC_READ: u32 = 2;

const fn ioc(dir: u32, ty: u32, nr: u32, size: u32) -> libc::c_ulong {
    ((dir << IOC_DIRSHIFT) | (ty << IOC_TYPESHIFT) | (nr << IOC_NRSHIFT) | (size << IOC_SIZESHIFT))
        as libc::c_ulong
}

const fn io(ty: u32, nr: u32) -> libc::c_ulong {
    ioc(IOC_NONE, ty, nr, 0)
}

const fn iow(ty: u32, nr: u32, size: u32) -> libc::c_ulong {
    ioc(IOC_WRITE, ty, nr, size)
}

const fn ior(ty: u32, nr: u32, size: u32) -> libc::c_ulong {
    ioc(IOC_READ, ty, nr, size)
}

const UI_DEV_CREATE: libc::c_ulong = io(b'U' as u32, 1);
const UI_DEV_DESTROY: libc::c_ulong = io(b'U' as u32, 2);
const UI_SET_EVBIT: libc::c_ulong = iow(b'U' as u32, 100, size_of::<libc::c_int>() as u32);
const UI_SET_KEYBIT: libc::c_ulong = iow(b'U' as u32, 101, size_of::<libc::c_int>() as u32);
const UI_SET_RELBIT: libc::c_ulong = iow(b'U' as u32, 102, size_of::<libc::c_int>() as u32);
const UI_SET_ABSBIT: libc::c_ulong = iow(b'U' as u32, 103, size_of::<libc::c_int>() as u32);

const EVIOCGID: libc::c_ulong = ior(b'E' as u32, 0x02, size_of::<libc::input_id>() as u32);
const EVIOCGNAME_LEN: usize = 256;
const EVIOCGNAME: libc::c_ulong = ior(b'E' as u32, 0x06, EVIOCGNAME_LEN as u32);

#[derive(Debug)]
pub struct EventProxyController {
    transport_config: crate::config::TransportConfig,
    virtual_input: Mutex<Option<VirtualInput>>,
}

#[derive(Debug)]
enum VirtualInput {
    Gamepad(UInputDevice),
    Mouse(UInputDevice),
}

impl EventProxyController {
    #[must_use]
    pub fn new(transport_config: crate::config::TransportConfig) -> Self {
        Self {
            transport_config,
            virtual_input: Mutex::new(None),
        }
    }

    fn wait_for_consumer(
        &self,
        handle: &Handle,
        tls: Option<givc_client::endpoint::TlsConfig>,
    ) -> Result<ClientEndpointConfig> {
        let endpoint = endpoint_from_transport(&self.transport_config, tls)?;
        let deadline = Instant::now() + Duration::from_secs(60);

        loop {
            if Instant::now() >= deadline {
                bail!("event: consumer not ready");
            }

            let connect = handle.block_on(endpoint.connect());
            match connect {
                Ok(_) => {
                    info!("event: successfully connected to consumer");
                    return Ok(endpoint);
                }
                Err(err) => {
                    warn!(error = %err, "event: consumer not ready, retrying in 1 second");
                    thread::sleep(Duration::from_secs(1));
                }
            }
        }
    }

    fn open_and_extract(&self, handler: &Path) -> Result<(File, pb::eventproxy::DeviceInfo)> {
        let file = OpenOptions::new()
            .read(true)
            .write(false)
            .open(handler)
            .with_context(|| format!("event: failed to open input device {}", handler.display()))?;

        let device_info = extract_device_info(&file).with_context(|| {
            format!(
                "event: failed to extract device info from {}",
                handler.display()
            )
        })?;

        Ok((file, device_info))
    }

    fn monitor_input_device(&self, target_device: &str) -> Result<PathBuf> {
        info!(target = %target_device, "event: monitoring input devices");

        loop {
            for entry in fs::read_dir("/dev/input").context("event: failed to read /dev/input")? {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(_) => continue,
                };
                let path = entry.path();
                let base = match path.file_name().and_then(|s| s.to_str()) {
                    Some(base) if base.starts_with("event") => base,
                    _ => continue,
                };
                let name = match input_device_name(&path) {
                    Ok(name) => name,
                    Err(_) => continue,
                };
                let lower = name.to_lowercase();
                if lower.contains("vmmouse") {
                    continue;
                }
                if lower.contains(&target_device.to_lowercase()) {
                    info!(device = %name, path = %path.display(), "event: device attached");
                    return Ok(path);
                }
                let _ = base;
            }

            thread::sleep(Duration::from_secs(1));
        }
    }

    fn close(&self) -> Result<()> {
        let mut guard = self
            .virtual_input
            .lock()
            .expect("event proxy mutex poisoned");
        *guard = None;
        Ok(())
    }

    fn register_device(&self, info: &pb::eventproxy::DeviceInfo) -> Result<()> {
        let name = info.name.to_lowercase();
        let mut guard = self
            .virtual_input
            .lock()
            .expect("event proxy mutex poisoned");

        let device = if name.contains("wireless controller") {
            UInputDevice::new_gamepad(UINPUT_NAME_GAMEPAD)?
        } else if name.contains("mouse") {
            UInputDevice::new_mouse(UINPUT_NAME_MOUSE)?
        } else {
            bail!("unsupported device");
        };

        *guard = Some(if name.contains("mouse") {
            VirtualInput::Mouse(device)
        } else {
            VirtualInput::Gamepad(device)
        });

        info!(
            device = %name,
            vendor = info.vendor_id,
            product = info.device_id,
            "event: registered virtual device"
        );
        Ok(())
    }

    fn stream_event(&self, event: &pb::eventproxy::InputEvent) -> Result<()> {
        let mut guard = self
            .virtual_input
            .lock()
            .expect("event proxy mutex poisoned");
        let Some(device) = guard.as_mut() else {
            bail!("event: no registered virtual device");
        };

        match device {
            VirtualInput::Gamepad(dev) | VirtualInput::Mouse(dev) => {
                dev.emit(event.r#type as u16, event.code as u16, event.value)?;
            }
        }

        Ok(())
    }
}

#[derive(Debug)]
pub struct EventProxyServer {
    controller: EventProxyController,
}

impl EventProxyServer {
    #[must_use]
    pub fn new(transport: crate::config::TransportConfig) -> Self {
        Self {
            controller: EventProxyController::new(transport),
        }
    }
}

#[tonic::async_trait]
impl pb::eventproxy::event_service_server::EventService for EventProxyServer {
    async fn register_device(
        &self,
        request: Request<pb::eventproxy::DeviceInfo>,
    ) -> Result<Response<pb::eventproxy::Ack>, Status> {
        let info = request.into_inner();
        self.controller.register_device(&info).map_err(map_err)?;
        Ok(Response::new(pb::eventproxy::Ack {
            status: "OK".to_owned(),
        }))
    }

    async fn stream_events(
        &self,
        request: Request<tonic::Streaming<pb::eventproxy::InputEvent>>,
    ) -> Result<Response<pb::eventproxy::Ack>, Status> {
        let mut stream = request.into_inner();
        loop {
            match stream.next().await {
                Some(Ok(event)) => {
                    self.controller.stream_event(&event).map_err(map_err)?;
                }
                Some(Err(err)) => {
                    let _ = self.controller.close();
                    return Err(Status::internal(err.to_string()));
                }
                None => {
                    return Ok(Response::new(pb::eventproxy::Ack {
                        status: "OK".to_owned(),
                    }));
                }
            }
        }
    }
}

pub async fn start_event_proxy_services(config: &AgentConfig) -> Result<()> {
    if !config.capabilities.event_proxy.enabled {
        return Ok(());
    }

    for event in &config.capabilities.event_proxy.events {
        start_event_proxy_service(config, event.clone()).await?;
    }

    Ok(())
}

async fn start_event_proxy_service(config: &AgentConfig, event: EventConfig) -> Result<()> {
    let controller = EventProxyController::new(event.transport.clone());

    if !event.producer {
        let listen_addr = event_server_addr(config, &event)?;
        let grpc_service = EventProxyServerServer::new(EventProxyServer { controller });
        tokio::spawn(async move {
            info!(addr = %listen_addr, device = %event.device, "event: server starting");
            if let Err(err) = Server::builder()
                .add_service(grpc_service)
                .serve(listen_addr)
                .await
            {
                error!(error = %err, "event: server failed");
            }
        });
        return Ok(());
    }

    let handle = Handle::current();
    let config = config.clone();
    thread::spawn(move || {
        // Keep Go's producer/server split: capture and injection run as independent workers so a
        // restart in one role does not stall the other.
        loop {
            let result = run_event_proxy_producer(&handle, &config, &event);
            match result {
                Ok(()) => return,
                Err(err) if err.to_string().contains("device disconnected") => {
                    warn!(error = %err, "event: client stream exited with disconnect, retrying");
                    thread::sleep(Duration::from_secs(1));
                }
                Err(err) => {
                    error!(error = %err, "event: client stream exited");
                    return;
                }
            }
        }
    });

    Ok(())
}

fn run_event_proxy_producer(
    handle: &Handle,
    config: &AgentConfig,
    event: &EventConfig,
) -> Result<()> {
    let controller = EventProxyController::new(event.transport.clone());
    let handler = controller.monitor_input_device(&event.device)?;
    let (mut dev, device_info) = controller.open_and_extract(&handler)?;

    let endpoint = controller.wait_for_consumer(handle, config.network.tls_config.clone())?;
    handle.block_on(async {
        let channel = endpoint.connect().await?;
        let mut client = pb::eventproxy::event_service_client::EventServiceClient::new(channel);

        client.register_device(device_info.clone()).await?;

        let (tx, rx) = mpsc::channel::<pb::eventproxy::InputEvent>(32);
        let stream = ReceiverStream::new(rx);
        let send_thread = thread::spawn(move || stream_device_events(&mut dev, tx));

        let response = client
            .stream_events(Request::new(stream))
            .await?
            .into_inner();
        let _ = send_thread.join();
        info!(status = %response.status, "event: producer stream completed");
        Ok::<_, anyhow::Error>(())
    })?;

    Ok(())
}

fn stream_device_events(
    dev: &mut File,
    tx: mpsc::Sender<pb::eventproxy::InputEvent>,
) -> Result<()> {
    loop {
        match read_input_event(dev) {
            Ok(event) => {
                if tx.blocking_send(input_event_to_message(&event)).is_err() {
                    return Ok(());
                }
            }
            Err(err) if is_device_disconnect_io(&err) => {
                return Err(anyhow::Error::msg("device disconnected"));
            }
            Err(_) => {
                thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

fn event_server_addr(config: &AgentConfig, event: &EventConfig) -> Result<std::net::SocketAddr> {
    if event.transport.protocol != "tcp" {
        bail!("event server requires tcp transport");
    }

    let addr: std::net::IpAddr = event
        .transport
        .address
        .parse()
        .with_context(|| format!("invalid event address {}", event.transport.address))?;
    let port: u16 = event
        .transport
        .port
        .parse()
        .with_context(|| format!("invalid event port {}", event.transport.port))?;

    let _ = config;
    Ok(std::net::SocketAddr::new(addr, port))
}

fn endpoint_from_transport(
    transport: &crate::config::TransportConfig,
    tls: Option<givc_client::endpoint::TlsConfig>,
) -> Result<ClientEndpointConfig> {
    let address = match transport.protocol.as_str() {
        "tcp" => EndpointAddress::Tcp {
            addr: transport.address.clone(),
            port: transport
                .port
                .parse()
                .with_context(|| format!("invalid tcp port {}", transport.port))?,
        },
        "unix" => EndpointAddress::Unix(transport.address.clone()),
        "abstract" => EndpointAddress::Abstract(transport.address.clone()),
        "vsock" => EndpointAddress::Vsock(tokio_vsock::VsockAddr::new(
            transport
                .address
                .parse()
                .with_context(|| format!("invalid vsock cid {}", transport.address))?,
            transport
                .port
                .parse()
                .with_context(|| format!("invalid vsock port {}", transport.port))?,
        )),
        other => bail!("unsupported event transport protocol: {other}"),
    };

    Ok(ClientEndpointConfig {
        transport: CommonTransportConfig {
            address,
            tls_name: transport.name.clone(),
        },
        tls,
    })
}

fn extract_device_info(file: &File) -> Result<pb::eventproxy::DeviceInfo> {
    let id: libc::input_id = ioctl_read_struct(file.as_raw_fd(), EVIOCGID)?;
    let name = input_device_name_from_fd(file.as_raw_fd())?;

    Ok(pb::eventproxy::DeviceInfo {
        device_id: u32::from(id.product),
        vendor_id: u32::from(id.vendor),
        name,
    })
}

fn input_device_name(path: &Path) -> Result<String> {
    let file = OpenOptions::new().read(true).open(path)?;
    input_device_name_from_fd(file.as_raw_fd())
}

fn input_device_name_from_fd(fd: libc::c_int) -> Result<String> {
    let mut buf = [0_u8; EVIOCGNAME_LEN];
    let rc = unsafe { libc::ioctl(fd, EVIOCGNAME, buf.as_mut_ptr()) };
    if rc < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    Ok(String::from_utf8_lossy(&buf[..end]).to_string())
}

fn read_input_event(dev: &mut File) -> std::io::Result<libc::input_event> {
    let mut buf = vec![0_u8; size_of::<libc::input_event>()];
    dev.read_exact(&mut buf)?;
    let event = unsafe { std::ptr::read_unaligned(buf.as_ptr() as *const libc::input_event) };
    Ok(event)
}

fn input_event_to_message(event: &libc::input_event) -> pb::eventproxy::InputEvent {
    let timestamp = timeval_to_nanos(&event.time);
    pb::eventproxy::InputEvent {
        timestamp,
        r#type: u32::from(event.type_),
        code: u32::from(event.code),
        value: event.value,
    }
}

fn timeval_to_nanos(tv: &libc::timeval) -> i64 {
    (tv.tv_sec as i64)
        .saturating_mul(1_000_000_000)
        .saturating_add((tv.tv_usec as i64).saturating_mul(1_000))
}

fn is_device_disconnect_io(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::BrokenPipe
    ) || err.raw_os_error() == Some(libc::ENODEV)
}

fn map_err(err: anyhow::Error) -> Status {
    Status::internal(err.to_string())
}

fn ioctl_read_struct<T: Copy>(fd: libc::c_int, req: libc::c_ulong) -> Result<T> {
    let mut out: T = unsafe { zeroed() };
    let rc = unsafe { libc::ioctl(fd, req, &mut out) };
    if rc < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(out)
}

#[derive(Debug)]
struct UInputDevice {
    file: File,
}

impl UInputDevice {
    fn new_mouse(name: &str) -> Result<Self> {
        Self::new(name, |dev| {
            dev.enable_evbit(EV_KEY)?;
            dev.enable_evbit(EV_REL)?;
            dev.enable_keybit(BTN_LEFT)?;
            dev.enable_keybit(BTN_RIGHT)?;
            dev.enable_keybit(BTN_MIDDLE)?;
            dev.enable_relbit(REL_X)?;
            dev.enable_relbit(REL_Y)?;
            dev.enable_relbit(REL_WHEEL)?;
            Ok(())
        })
    }

    fn new_gamepad(name: &str) -> Result<Self> {
        Self::new(name, |dev| {
            dev.enable_evbit(EV_KEY)?;
            dev.enable_evbit(EV_ABS)?;

            for code in [
                BTN_SOUTH,
                BTN_EAST,
                BTN_C,
                BTN_NORTH,
                BTN_WEST,
                BTN_TL,
                BTN_TR,
                BTN_SELECT,
                BTN_START,
                BTN_MODE,
                BTN_THUMBL,
                BTN_THUMBR,
                BTN_DPAD_UP,
                BTN_DPAD_DOWN,
                BTN_DPAD_LEFT,
                BTN_DPAD_RIGHT,
            ] {
                dev.enable_keybit(code)?;
            }

            for code in [
                ABS_X, ABS_Y, ABS_Z, ABS_RX, ABS_RY, ABS_RZ, ABS_HAT0X, ABS_HAT0Y,
            ] {
                dev.enable_absbit(code)?;
            }

            dev.set_abs_range(ABS_X, -32768, 32767)?;
            dev.set_abs_range(ABS_Y, -32768, 32767)?;
            dev.set_abs_range(ABS_RX, -32768, 32767)?;
            dev.set_abs_range(ABS_RY, -32768, 32767)?;
            dev.set_abs_range(ABS_Z, 0, 255)?;
            dev.set_abs_range(ABS_RZ, 0, 255)?;
            dev.set_abs_range(ABS_HAT0X, -1, 1)?;
            dev.set_abs_range(ABS_HAT0Y, -1, 1)?;
            Ok(())
        })
    }

    fn new(name: &str, setup: impl FnOnce(&mut UInputDeviceBuilder) -> Result<()>) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/uinput")
            .context("event: unable to open /dev/uinput")?;

        let mut builder = UInputDeviceBuilder::new(file);
        setup(&mut builder)?;
        builder.create(name)
    }

    fn emit(&mut self, event_type: u16, code: u16, value: i32) -> Result<()> {
        self.write_event(event_type, code, value)?;
        self.sync()?;
        Ok(())
    }

    fn write_event(&mut self, event_type: u16, code: u16, value: i32) -> Result<()> {
        let event = unsafe {
            let mut event: libc::input_event = zeroed();
            event.type_ = event_type;
            event.code = code;
            event.value = value;
            event
        };
        self.file.write_all(as_bytes(&event))?;
        Ok(())
    }

    fn sync(&mut self) -> Result<()> {
        self.write_event(EV_SYN, SYN_REPORT, 0)
    }
}

impl Drop for UInputDevice {
    fn drop(&mut self) {
        let _ = unsafe { libc::ioctl(self.file.as_raw_fd(), UI_DEV_DESTROY) };
    }
}

struct UInputDeviceBuilder {
    file: File,
    abs: [libc::input_absinfo; 64],
}

impl UInputDeviceBuilder {
    fn new(file: File) -> Self {
        Self {
            file,
            abs: unsafe { zeroed() },
        }
    }

    fn enable_evbit(&mut self, bit: u16) -> Result<()> {
        ioctl_set_int(self.file.as_raw_fd(), UI_SET_EVBIT, bit as i32)
    }

    fn enable_keybit(&mut self, bit: u16) -> Result<()> {
        ioctl_set_int(self.file.as_raw_fd(), UI_SET_KEYBIT, bit as i32)
    }

    fn enable_relbit(&mut self, bit: u16) -> Result<()> {
        ioctl_set_int(self.file.as_raw_fd(), UI_SET_RELBIT, bit as i32)
    }

    fn enable_absbit(&mut self, bit: u16) -> Result<()> {
        ioctl_set_int(self.file.as_raw_fd(), UI_SET_ABSBIT, bit as i32)
    }

    fn set_abs_range(&mut self, axis: u16, min: i32, max: i32) -> Result<()> {
        let slot = axis as usize;
        self.abs[slot].minimum = min;
        self.abs[slot].maximum = max;
        Ok(())
    }

    fn create(mut self, name: &str) -> Result<UInputDevice> {
        let mut device: libc::uinput_user_dev = unsafe { zeroed() };
        let name_bytes = CString::new(name).context("event: invalid device name")?;
        let name_bytes = name_bytes.as_bytes_with_nul();
        let copy_len = name_bytes.len().min(device.name.len());
        for (dst, src) in device
            .name
            .iter_mut()
            .zip(name_bytes.iter().copied())
            .take(copy_len)
        {
            *dst = src as libc::c_char;
        }
        device.id = libc::input_id {
            bustype: BUS_USB,
            vendor: 0x0001,
            product: 0x0001,
            version: 1,
        };
        device.ff_effects_max = 0;
        device.absmin = self.abs.map(|a| a.minimum);
        device.absmax = self.abs.map(|a| a.maximum);

        self.file.write_all(as_bytes(&device))?;
        let rc = unsafe { libc::ioctl(self.file.as_raw_fd(), UI_DEV_CREATE) };
        if rc < 0 {
            return Err(std::io::Error::last_os_error().into());
        }

        Ok(UInputDevice { file: self.file })
    }
}

fn ioctl_set_int(fd: libc::c_int, req: libc::c_ulong, value: i32) -> Result<()> {
    let rc = unsafe { libc::ioctl(fd, req, value) };
    if rc < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

fn as_bytes<T>(value: &T) -> &[u8] {
    unsafe { std::slice::from_raw_parts((value as *const T).cast::<u8>(), size_of::<T>()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn encodes_message() {
        let msg = input_event_to_message(&libc::input_event {
            time: libc::timeval {
                tv_sec: 1,
                tv_usec: 2,
            },
            type_: EV_KEY,
            code: BTN_LEFT,
            value: 1,
        });
        assert_eq!(msg.r#type, u32::from(EV_KEY));
        assert_eq!(msg.code, u32::from(BTN_LEFT));
        assert_eq!(msg.timestamp, 1_000_002_000);
    }
}
