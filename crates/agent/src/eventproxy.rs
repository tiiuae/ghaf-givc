// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use evdev::uinput::VirtualDevice;
use evdev::{
    AbsInfo, AbsoluteAxisCode, AttributeSet, BusType, Device, InputEvent, InputId, KeyCode,
    RelativeAxisCode, UinputAbsSetup,
};
use givc_client::endpoint::EndpointConfig as ClientEndpointConfig;
use givc_common::address::EndpointAddress;
use givc_common::pb::reflection::EVENT_DESCRIPTOR;
use givc_common::types::TransportConfig as CommonTransportConfig;
use givc_common::{pb, tonic::RewrapErr};
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

    fn open_and_extract(&self, handler: &Path) -> Result<(Device, pb::eventproxy::DeviceInfo)> {
        let device = Device::open(handler)
            .with_context(|| format!("event: failed to open input device {}", handler.display()))?;

        let device_info = extract_device_info(&device).with_context(|| {
            format!(
                "event: failed to extract device info from {}",
                handler.display()
            )
        })?;

        Ok((device, device_info))
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
                let name = match Device::open(&path)
                    .ok()
                    .and_then(|device| device.name().map(str::to_owned))
                {
                    Some(name) => name,
                    None => continue,
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
        self.controller.register_device(&info).rewrap_err()?;
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
                    self.controller.stream_event(&event).rewrap_err()?;
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

    if !event.producer.enabled {
        let listen_addr = event_server_addr(config, &event)?;
        let grpc_service = EventProxyServerServer::new(EventProxyServer { controller });
        let reflect = tonic_reflection::server::Builder::configure()
            .register_encoded_file_descriptor_set(EVENT_DESCRIPTOR)
            .build_v1()?;
        let mut server = Server::builder();
        if let Some(tls) = &config.network.tls_config {
            server = server.tls_config(tls.server_config()?)?;
        }
        tokio::spawn(async move {
            info!(addr = %listen_addr, device = %event.device, "event: server starting");
            if let Err(err) = server
                .add_service(grpc_service)
                .add_service(reflect)
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
    dev: &mut Device,
    tx: mpsc::Sender<pb::eventproxy::InputEvent>,
) -> Result<()> {
    loop {
        match dev.fetch_events() {
            Ok(events) => {
                for event in events {
                    if tx.blocking_send(input_event_to_message(&event)).is_err() {
                        return Ok(());
                    }
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
    ensure!(
        event.transport.protocol == "tcp",
        "event server requires tcp transport"
    );

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
    let mut tls = tls;
    if let Some(tls_config) = tls.as_mut() {
        tls_config.tls_name = Some(transport.name.clone());
    }
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

fn extract_device_info(device: &Device) -> Result<pb::eventproxy::DeviceInfo> {
    let id = device.input_id();
    let name = device.name().unwrap_or_default().to_owned();

    Ok(pb::eventproxy::DeviceInfo {
        device_id: u32::from(id.product()),
        vendor_id: u32::from(id.vendor()),
        name,
    })
}

fn input_event_to_message(event: &InputEvent) -> pb::eventproxy::InputEvent {
    let timestamp = event
        .timestamp()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos().min(i64::MAX as u128) as i64)
        .unwrap_or_default();
    pb::eventproxy::InputEvent {
        timestamp,
        r#type: u32::from(event.event_type().0),
        code: u32::from(event.code()),
        value: event.value(),
    }
}

fn is_device_disconnect_io(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::BrokenPipe
    ) || err.raw_os_error() == Some(libc::ENODEV)
}

#[derive(Debug)]
struct UInputDevice {
    device: VirtualDevice,
}

impl UInputDevice {
    fn new_mouse(name: &str) -> Result<Self> {
        let mut keys = AttributeSet::<KeyCode>::new();
        for key in [KeyCode::BTN_LEFT, KeyCode::BTN_RIGHT, KeyCode::BTN_MIDDLE] {
            keys.insert(key);
        }
        let mut axes = AttributeSet::<RelativeAxisCode>::new();
        for axis in [
            RelativeAxisCode::REL_X,
            RelativeAxisCode::REL_Y,
            RelativeAxisCode::REL_WHEEL,
        ] {
            axes.insert(axis);
        }
        let device = VirtualDevice::builder()?
            .name(name)
            .input_id(InputId::new(BusType::BUS_USB, 1, 1, 1))
            .with_keys(&keys)?
            .with_relative_axes(&axes)?
            .build()?;
        Ok(Self { device })
    }

    fn new_gamepad(name: &str) -> Result<Self> {
        let mut keys = AttributeSet::<KeyCode>::new();
        for key in [
            KeyCode::BTN_SOUTH,
            KeyCode::BTN_EAST,
            KeyCode::BTN_C,
            KeyCode::BTN_NORTH,
            KeyCode::BTN_WEST,
            KeyCode::BTN_TL,
            KeyCode::BTN_TR,
            KeyCode::BTN_SELECT,
            KeyCode::BTN_START,
            KeyCode::BTN_MODE,
            KeyCode::BTN_THUMBL,
            KeyCode::BTN_THUMBR,
            KeyCode::BTN_DPAD_UP,
            KeyCode::BTN_DPAD_DOWN,
            KeyCode::BTN_DPAD_LEFT,
            KeyCode::BTN_DPAD_RIGHT,
        ] {
            keys.insert(key);
        }
        let axes = [
            (AbsoluteAxisCode::ABS_X, -32768, 32767),
            (AbsoluteAxisCode::ABS_Y, -32768, 32767),
            (AbsoluteAxisCode::ABS_RX, -32768, 32767),
            (AbsoluteAxisCode::ABS_RY, -32768, 32767),
            (AbsoluteAxisCode::ABS_Z, 0, 255),
            (AbsoluteAxisCode::ABS_RZ, 0, 255),
            (AbsoluteAxisCode::ABS_HAT0X, -1, 1),
            (AbsoluteAxisCode::ABS_HAT0Y, -1, 1),
        ];
        let mut builder = VirtualDevice::builder()?
            .name(name)
            .input_id(InputId::new(BusType::BUS_USB, 1, 1, 1))
            .with_keys(&keys)?;
        for (axis, min, max) in axes {
            builder = builder.with_absolute_axis(&UinputAbsSetup::new(
                axis,
                AbsInfo::new(0, min, max, 0, 0, 0),
            ))?;
        }
        Ok(Self {
            device: builder.build()?,
        })
    }

    fn emit(&mut self, event_type: u16, code: u16, value: i32) -> Result<()> {
        self.device
            .emit(&[InputEvent::new(event_type, code, value)])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn encodes_message() {
        let msg = input_event_to_message(&InputEvent::new(
            evdev::EventType::KEY.0,
            KeyCode::BTN_LEFT.0,
            1,
        ));
        assert_eq!(msg.r#type, u32::from(evdev::EventType::KEY.0));
        assert_eq!(msg.code, u32::from(KeyCode::BTN_LEFT.0));
        assert_eq!(msg.timestamp, 0);
    }

    #[test]
    fn endpoint_propagates_tls_name() {
        let transport = crate::config::TransportConfig {
            protocol: "tcp".to_owned(),
            address: "192.0.2.20".to_owned(),
            port: "9014".to_owned(),
            name: "gui-vm".to_owned(),
        };
        let tls = Some(givc_client::endpoint::TlsConfig {
            ca_cert_file_path: Default::default(),
            cert_file_path: Default::default(),
            key_file_path: Default::default(),
            tls_name: None,
        });

        let endpoint = endpoint_from_transport(&transport, tls).unwrap();

        assert_eq!(endpoint.transport.tls_name, "gui-vm");
        assert_eq!(
            endpoint
                .tls
                .as_ref()
                .and_then(|tls| tls.tls_name.as_deref()),
            Some("gui-vm")
        );
    }
}
