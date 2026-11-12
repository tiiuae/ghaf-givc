// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
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
use tokio_listener::{Listener, ListenerAddress, SystemOptions, UserOptions};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tonic_middleware::RequestInterceptorLayer;
use tracing::{error, info, warn};

use crate::config::{AgentConfig, EventConfig};
use crate::runtime::AgentInterceptor;

pub use pb::eventproxy::event_service_server::EventServiceServer as EventProxyServerServer;

const UINPUT_NAME_MOUSE: &str = "Generic Mouse";
const UINPUT_NAME_GAMEPAD: &str = "Microsoft X-Box One S pad";
const SYN_REPORT: u16 = evdev::SynchronizationCode::SYN_REPORT.0;

#[derive(Debug)]
struct GamepadProfile {
    input_id: InputId,
    trigger_max: i32,
    stick_flat: i32,
    stick_fuzz: i32,
}

fn gamepad_profile() -> GamepadProfile {
    GamepadProfile {
        input_id: InputId::new(BusType::BUS_USB, 0x045e, 0x02ea, 0x0408),
        trigger_max: 1023,
        stick_flat: 128,
        stick_fuzz: 16,
    }
}

fn gamepad_axis_setups() -> Vec<UinputAbsSetup> {
    let profile = gamepad_profile();
    [
        (AbsoluteAxisCode::ABS_X, -32768, 32767),
        (AbsoluteAxisCode::ABS_Y, -32768, 32767),
        (AbsoluteAxisCode::ABS_RX, -32768, 32767),
        (AbsoluteAxisCode::ABS_RY, -32768, 32767),
        (AbsoluteAxisCode::ABS_Z, 0, profile.trigger_max),
        (AbsoluteAxisCode::ABS_RZ, 0, profile.trigger_max),
        (AbsoluteAxisCode::ABS_HAT0X, -1, 1),
        (AbsoluteAxisCode::ABS_HAT0Y, -1, 1),
    ]
    .into_iter()
    .map(|(axis, min, max)| {
        let stick_axis = is_stick_axis(axis);
        UinputAbsSetup::new(
            axis,
            AbsInfo::new(
                0,
                min,
                max,
                if stick_axis { profile.stick_fuzz } else { 0 },
                if stick_axis { profile.stick_flat } else { 0 },
                0,
            ),
        )
    })
    .collect()
}

fn mouse_input_id() -> InputId {
    InputId::new(BusType::BUS_USB, 0xdead, 0xbeef, 1)
}

fn is_syn_report(event: &InputEvent) -> bool {
    event.event_type() == evdev::EventType::SYNCHRONIZATION && event.code() == SYN_REPORT
}

fn split_event_frames(events: &[InputEvent]) -> Vec<Vec<InputEvent>> {
    let mut frames = Vec::new();
    let mut frame = Vec::new();
    for event in events {
        if is_syn_report(event) {
            frames.push(std::mem::take(&mut frame));
        } else {
            frame.push(*event);
        }
    }
    if !frame.is_empty() {
        frames.push(frame);
    }
    frames
}
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
        shutdown: &mut crate::runtime::AgentShutdown,
    ) -> Result<ClientEndpointConfig> {
        let endpoint = endpoint_from_transport(&self.transport_config, tls)?;
        let deadline = Instant::now() + Duration::from_secs(60);

        loop {
            if shutdown.is_cancelled() {
                bail!("event: shutdown while waiting for consumer");
            }
            if Instant::now() >= deadline {
                bail!("event: consumer not ready");
            }

            let connect = handle.block_on(async {
                tokio::select! {
                    () = shutdown.cancelled() => None,
                    result = endpoint.connect() => Some(result),
                }
            });
            let Some(connect) = connect else {
                bail!("event: shutdown while connecting to consumer");
            };
            match connect {
                Ok(_) => {
                    info!("event: successfully connected to consumer");
                    return Ok(endpoint);
                }
                Err(err) => {
                    warn!(error = %err, "event: consumer not ready, retrying in 1 second");
                    let cancelled = handle.block_on(async {
                        tokio::select! {
                            () = shutdown.cancelled() => true,
                            () = tokio::time::sleep(Duration::from_secs(1)) => false,
                        }
                    });
                    if cancelled {
                        bail!("event: shutdown while waiting for consumer");
                    }
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

    fn monitor_input_device(
        &self,
        target_device: &str,
        shutdown: &crate::runtime::AgentShutdown,
    ) -> Result<PathBuf> {
        info!(target = %target_device, "event: monitoring input devices");

        loop {
            if shutdown.is_cancelled() {
                bail!("event: shutdown while monitoring input devices");
            }
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

    fn stream_frame(&self, events: &[InputEvent]) -> Result<()> {
        let mut guard = self
            .virtual_input
            .lock()
            .expect("event proxy mutex poisoned");
        let Some(device) = guard.as_mut() else {
            bail!("event: no registered virtual device");
        };

        match device {
            VirtualInput::Gamepad(dev) | VirtualInput::Mouse(dev) => dev.emit(events)?,
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
        let mut frame = Vec::new();
        loop {
            match stream.next().await {
                Some(Ok(event)) => {
                    let input_event =
                        InputEvent::new(event.r#type as u16, event.code as u16, event.value);
                    if is_syn_report(&input_event) {
                        frame.push(input_event);
                        for completed_frame in split_event_frames(&frame) {
                            self.controller
                                .stream_frame(&completed_frame)
                                .rewrap_err()?;
                        }
                        frame.clear();
                    } else {
                        frame.push(input_event);
                    }
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

pub(crate) async fn start_event_proxy_services(
    config: &AgentConfig,
    shutdown: crate::runtime::AgentShutdown,
) -> Result<Vec<tokio::task::JoinHandle<()>>> {
    if !config.capabilities.event_proxy.enabled {
        return Ok(Vec::new());
    }
    if shutdown.is_cancelled() {
        return Ok(Vec::new());
    }

    let mut workers = Vec::new();
    for event in &config.capabilities.event_proxy.events {
        if shutdown.is_cancelled() {
            break;
        }
        match start_event_proxy_service(config, event.clone(), shutdown.clone()).await {
            Ok(worker) => workers.push(worker),
            Err(err) => error!(error = %err, "event: unable to start configured proxy; continuing"),
        }
    }

    Ok(workers)
}

async fn start_event_proxy_service(
    config: &AgentConfig,
    event: EventConfig,
    shutdown: crate::runtime::AgentShutdown,
) -> Result<tokio::task::JoinHandle<()>> {
    let controller = EventProxyController::new(event.transport.clone());

    if !event.producer.enabled {
        let listen_addr = event_server_listener_address(&event)?;
        let grpc_service = EventProxyServerServer::new(EventProxyServer { controller });
        let reflect = tonic_reflection::server::Builder::configure()
            .register_encoded_file_descriptor_set(EVENT_DESCRIPTOR)
            .build_v1()?;
        let mut server = Server::builder().layer(RequestInterceptorLayer::new(
            AgentInterceptor::from_config(config)?,
        ));
        let tls_acceptor = config
            .network
            .tls_config
            .as_ref()
            .map(crate::runtime::tls_acceptor)
            .transpose()?;
        return Ok(tokio::spawn(async move {
            let listener = match Listener::bind(
                &listen_addr,
                &SystemOptions::default(),
                &UserOptions::default(),
            )
            .await
            .with_context(|| format!("unable to bind event listener {listen_addr:?}"))
            {
                Ok(listener) => listener,
                Err(err) => {
                    error!(error = %err, "event: server failed");
                    return;
                }
            };
            info!(addr = ?listen_addr, device = %event.device, "event: server starting");
            let server = server.add_service(grpc_service).add_service(reflect);
            let mut shutdown = shutdown;
            let result = if let Some(acceptor) = tls_acceptor {
                server
                    .serve_with_incoming_shutdown(
                        crate::runtime::tls_incoming(listener, acceptor),
                        shutdown.cancelled(),
                    )
                    .await
            } else {
                server
                    .serve_with_incoming_shutdown(listener, shutdown.cancelled())
                    .await
            };
            if let Err(err) = result {
                error!(error = %err, "event: server failed");
            }
        }));
    }

    let handle = Handle::current();
    let config = config.clone();
    let worker_shutdown = shutdown.clone();
    let worker = tokio::task::spawn_blocking(move || {
        let mut worker_shutdown = worker_shutdown;
        // Keep Go's producer/server split: capture and injection run as independent workers so a
        // restart in one role does not stall the other.
        loop {
            if worker_shutdown.is_cancelled() {
                return;
            }
            let result =
                run_event_proxy_producer(&handle, &config, &event, worker_shutdown.clone());
            match result {
                Ok(()) => return,
                Err(err) => {
                    warn!(error = %err, "event: producer stopped, retrying device discovery and registration");
                    if wait_event_producer_retry(&handle, &mut worker_shutdown) {
                        return;
                    }
                }
            }
        }
    });

    Ok(tokio::spawn(async move {
        if let Err(err) = worker.await {
            error!(error = %err, "event: producer worker failed");
        }
    }))
}

fn wait_event_producer_retry(
    handle: &Handle,
    shutdown: &mut crate::runtime::AgentShutdown,
) -> bool {
    handle.block_on(async {
        tokio::select! {
            () = shutdown.cancelled() => true,
            () = tokio::time::sleep(Duration::from_secs(1)) => false,
        }
    })
}

fn run_event_proxy_producer(
    handle: &Handle,
    config: &AgentConfig,
    event: &EventConfig,
    mut shutdown: crate::runtime::AgentShutdown,
) -> Result<()> {
    let controller = EventProxyController::new(event.transport.clone());
    let handler = controller.monitor_input_device(&event.device, &shutdown)?;
    let (mut dev, device_info) = controller.open_and_extract(&handler)?;
    dev.set_nonblocking(true)
        .context("unable to set event device nonblocking")?;

    let endpoint =
        controller.wait_for_consumer(handle, config.network.tls_config.clone(), &mut shutdown)?;
    handle.block_on(async {
        let channel = tokio::select! {
            () = shutdown.cancelled() => return Ok(()),
            result = endpoint.connect() => result?,
        };
        let mut client = pb::eventproxy::event_service_client::EventServiceClient::new(channel);

        tokio::select! {
            () = shutdown.cancelled() => return Ok(()),
            result = client.register_device(device_info.clone()) => { result?; },
        }

        let (tx, rx) = mpsc::channel::<pb::eventproxy::InputEvent>(32);
        let stream = ReceiverStream::new(rx);
        let stopping = Arc::new(AtomicBool::new(false));
        let reader_stopping = stopping.clone();
        let send_thread =
            thread::spawn(move || stream_device_events(&mut dev, tx, reader_stopping));
        let response = match finish_event_reader_and_rpc(
            send_thread,
            client.stream_events(Request::new(stream)),
            &mut shutdown,
            stopping,
        )
        .await?
        {
            Some(response) => response.into_inner(),
            None => return Ok(()),
        };
        info!(status = %response.status, "event: producer stream completed");
        Ok::<_, anyhow::Error>(())
    })?;

    Ok(())
}

fn stream_device_events(
    dev: &mut Device,
    tx: mpsc::Sender<pb::eventproxy::InputEvent>,
    stopping: Arc<AtomicBool>,
) -> Result<()> {
    loop {
        if stopping.load(Ordering::Acquire) {
            return Ok(());
        }
        match dev.fetch_events() {
            Ok(events) => {
                for event in events {
                    if stopping.load(Ordering::Acquire) {
                        return Ok(());
                    }
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

async fn finish_event_reader_and_rpc<F, T>(
    reader: thread::JoinHandle<Result<()>>,
    rpc: F,
    shutdown: &mut crate::runtime::AgentShutdown,
    stopping: Arc<AtomicBool>,
) -> Result<Option<T>>
where
    F: std::future::Future<Output = std::result::Result<T, Status>>,
{
    tokio::pin!(rpc);
    let mut reader = tokio::task::spawn_blocking(move || reader.join());
    let mut reader_result = None;
    let mut rpc_result = None;

    while reader_result.is_none() || rpc_result.is_none() {
        if shutdown.is_cancelled() {
            stopping.store(true, Ordering::Release);
            let deadline = shutdown
                .deadline()
                .unwrap_or_else(|| tokio::time::Instant::now() + Duration::from_secs(4));
            let drained = tokio::time::timeout_at(deadline, async {
                while reader_result.is_none() || rpc_result.is_none() {
                    tokio::select! {
                        result = &mut reader, if reader_result.is_none() => reader_result = Some(result),
                        result = &mut rpc, if rpc_result.is_none() => {
                            if result.is_err() {
                                stopping.store(true, Ordering::Release);
                            }
                            rpc_result = Some(result);
                        }
                    }
                }
            })
            .await;
            if drained.is_err() {
                return Ok(None);
            }
            break;
        }

        tokio::select! {
            result = &mut reader, if reader_result.is_none() => reader_result = Some(result),
            result = &mut rpc, if rpc_result.is_none() => {
                if result.is_err() {
                    stopping.store(true, Ordering::Release);
                }
                rpc_result = Some(result);
            }
            () = shutdown.cancelled() => stopping.store(true, Ordering::Release),
        }
    }

    match reader_result
        .expect("reader loop completed")
        .context("event reader join task failed")?
    {
        Ok(Ok(())) => {}
        Ok(Err(err)) => return Err(err),
        Err(_) => bail!("event: device reader thread panicked"),
    }

    Ok(Some(rpc_result.expect("RPC loop completed")?))
}

fn event_server_listener_address(event: &EventConfig) -> Result<ListenerAddress> {
    let transport = &event.transport;
    match transport.protocol.as_str() {
        "tcp" => {
            let addr = transport
                .address
                .parse()
                .with_context(|| format!("invalid event address {}", transport.address))?;
            let port = transport
                .port
                .parse()
                .with_context(|| format!("invalid event port {}", transport.port))?;
            Ok(ListenerAddress::Tcp(std::net::SocketAddr::new(addr, port)))
        }
        "unix" => Ok(ListenerAddress::Path(transport.address.clone().into())),
        "abstract" => Ok(ListenerAddress::Abstract(transport.address.clone())),
        "vsock" => {
            let cid = transport.address.parse().with_context(|| {
                format!(
                    "invalid vsock address {}:{}",
                    transport.address, transport.port
                )
            })?;
            let port = transport.port.parse().with_context(|| {
                format!(
                    "invalid vsock address {}:{}",
                    transport.address, transport.port
                )
            })?;
            Ok(ListenerAddress::Vsock { cid, port })
        }
        other => bail!("unsupported event transport protocol: {other}"),
    }
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
            .input_id(mouse_input_id())
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
            KeyCode::BTN_NORTH,
            KeyCode::BTN_WEST,
            KeyCode::BTN_TL,
            KeyCode::BTN_TR,
            KeyCode::BTN_SELECT,
            KeyCode::BTN_START,
            KeyCode::BTN_MODE,
            KeyCode::BTN_THUMBL,
            KeyCode::BTN_THUMBR,
        ] {
            keys.insert(key);
        }
        let profile = gamepad_profile();
        let mut builder = VirtualDevice::builder()?
            .name(name)
            .input_id(profile.input_id)
            .with_keys(&keys)?;
        for axis in gamepad_axis_setups() {
            builder = builder.with_absolute_axis(&axis)?;
        }
        Ok(Self {
            device: builder.build()?,
        })
    }

    fn emit(&mut self, events: &[InputEvent]) -> Result<()> {
        self.device.emit(events)?;
        Ok(())
    }
}

fn is_stick_axis(axis: AbsoluteAxisCode) -> bool {
    matches!(
        axis,
        AbsoluteAxisCode::ABS_X
            | AbsoluteAxisCode::ABS_Y
            | AbsoluteAxisCode::ABS_RX
            | AbsoluteAxisCode::ABS_RY
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event_config(protocol: &str, address: &str, port: &str) -> EventConfig {
        EventConfig {
            transport: crate::config::TransportConfig {
                protocol: protocol.to_owned(),
                address: address.to_owned(),
                port: port.to_owned(),
                name: String::new(),
            },
            ..Default::default()
        }
    }

    fn event_listener_address(
        protocol: &str,
        address: &str,
        port: &str,
    ) -> Result<ListenerAddress> {
        event_server_listener_address(&event_config(protocol, address, port))
    }

    fn event(event_type: u16, code: u16, value: i32) -> InputEvent {
        InputEvent::new(event_type, code, value)
    }

    #[test]
    fn event_listener_accepts_tcp_transport() {
        let address = event_listener_address("tcp", "192.0.2.20", "9014").unwrap();

        assert!(matches!(
            address,
            ListenerAddress::Tcp(address)
                if address == "192.0.2.20:9014".parse::<std::net::SocketAddr>().unwrap()
        ));
    }

    #[test]
    fn event_listener_accepts_unix_transport() {
        let address = event_listener_address("unix", "/run/givc/event.sock", "").unwrap();

        assert!(matches!(
            address,
            ListenerAddress::Path(path) if path == Path::new("/run/givc/event.sock")
        ));
    }

    #[test]
    fn event_listener_accepts_abstract_transport() {
        let address = event_listener_address("abstract", "givc-event", "").unwrap();

        assert!(matches!(
            address,
            ListenerAddress::Abstract(name) if name == "givc-event"
        ));
    }

    #[test]
    fn event_listener_accepts_vsock_transport() {
        let address = event_listener_address("vsock", "3", "9014").unwrap();

        assert!(matches!(
            address,
            ListenerAddress::Vsock { cid: 3, port: 9014 }
        ));
    }

    #[test]
    fn event_listener_reports_invalid_tcp_address() {
        let error = event_listener_address("tcp", "not-an-ip", "9014").unwrap_err();

        assert!(error.to_string().contains("not-an-ip"));
    }

    #[test]
    fn event_listener_reports_invalid_vsock_address() {
        for (cid, port) in [("not-a-cid", "9014"), ("3", "not-a-port")] {
            let error = event_listener_address("vsock", cid, port).unwrap_err();
            assert!(error.to_string().contains("invalid vsock address"));
        }
    }

    #[test]
    fn event_listener_rejects_unsupported_protocol() {
        let error = event_listener_address("udp", "192.0.2.20", "9014").unwrap_err();

        assert!(
            error
                .to_string()
                .contains("unsupported event transport protocol: udp")
        );
    }

    #[tokio::test]
    async fn event_listener_bind_failure_does_not_abort_startup() {
        let occupied = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = occupied.local_addr().unwrap();
        let event = event_config(
            "tcp",
            &address.ip().to_string(),
            &address.port().to_string(),
        );

        let (_shutdown_trigger, shutdown) = crate::runtime::AgentShutdown::channel();
        let result = start_event_proxy_service(&AgentConfig::default(), event, shutdown).await;
        tokio::task::yield_now().await;

        assert!(result.is_ok(), "{result:?}");
    }

    #[tokio::test]
    async fn invalid_event_proxy_does_not_block_sibling_startup() {
        let mut config = AgentConfig::default();
        config.capabilities.event_proxy.enabled = true;
        config.capabilities.event_proxy.events = vec![
            event_config("udp", "127.0.0.1", "0"),
            event_config("tcp", "127.0.0.1", "0"),
        ];
        let (trigger, shutdown) = crate::runtime::AgentShutdown::channel();

        let workers = start_event_proxy_services(&config, shutdown).await.unwrap();

        assert_eq!(workers.len(), 1, "valid sibling must still start");
        trigger.trigger();
        for worker in workers {
            tokio::time::timeout(Duration::from_millis(200), worker)
                .await
                .expect("event listener must observe shutdown")
                .expect("event listener task must exit");
        }
    }

    #[tokio::test]
    async fn shutdown_interrupts_event_consumer_retry_wait() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port().to_string();
        drop(listener);
        let controller =
            EventProxyController::new(event_config("tcp", "127.0.0.1", &port).transport);
        let handle = Handle::current();
        let (trigger, mut shutdown) = crate::runtime::AgentShutdown::channel();
        let worker = tokio::task::spawn_blocking(move || {
            controller.wait_for_consumer(&handle, None, &mut shutdown)
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        trigger.trigger();

        let result = tokio::time::timeout(Duration::from_millis(200), worker)
            .await
            .expect("shutdown must interrupt event retry wait")
            .expect("event worker must finish");
        assert!(result.is_err(), "shutdown ends consumer discovery");
    }

    #[tokio::test]
    async fn event_reader_drain_polls_stream_while_joining_reader() {
        let (tx, mut rx) = mpsc::channel(1);
        let reader = thread::spawn(move || -> Result<()> {
            tx.blocking_send(1)?;
            tx.blocking_send(2)?;
            Ok(())
        });
        let rpc = async move {
            let mut received = 0;
            while rx.recv().await.is_some() {
                received += 1;
            }
            Ok::<_, Status>(received)
        };
        let (_trigger, mut shutdown) = crate::runtime::AgentShutdown::channel();

        let response = finish_event_reader_and_rpc(
            reader,
            rpc,
            &mut shutdown,
            Arc::new(AtomicBool::new(false)),
        )
        .await
        .unwrap();

        assert_eq!(response, Some(2));
    }

    #[tokio::test]
    async fn shutdown_drains_rpc_while_reader_channel_is_full() {
        let (tx, mut rx) = mpsc::channel(1);
        let reader = thread::spawn(move || -> Result<()> {
            tx.blocking_send(1)?;
            tx.blocking_send(2)?;
            Ok(())
        });
        let (resume_tx, resume_rx) = tokio::sync::oneshot::channel();
        let rpc = async move {
            let _ = resume_rx.await;
            let mut received = 0;
            while rx.recv().await.is_some() {
                received += 1;
            }
            Ok::<_, Status>(received)
        };
        let (trigger, mut shutdown) = crate::runtime::AgentShutdown::channel();
        let stopping = Arc::new(AtomicBool::new(false));
        let worker_stopping = stopping.clone();
        let worker = tokio::spawn(async move {
            finish_event_reader_and_rpc(reader, rpc, &mut shutdown, worker_stopping).await
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        trigger.trigger();
        resume_tx.send(()).unwrap();

        let response = tokio::time::timeout(Duration::from_millis(200), worker)
            .await
            .expect("reader and RPC must drain before deadline")
            .expect("drain task must finish")
            .unwrap();
        assert_eq!(response, Some(2));
        assert!(stopping.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn shutdown_interrupts_event_producer_retry_delay() {
        let handle = Handle::current();
        let (trigger, mut shutdown) = crate::runtime::AgentShutdown::channel();
        let waiter =
            tokio::task::spawn_blocking(move || wait_event_producer_retry(&handle, &mut shutdown));
        tokio::time::sleep(Duration::from_millis(20)).await;
        trigger.trigger();

        assert!(
            tokio::time::timeout(Duration::from_millis(200), waiter)
                .await
                .expect("shutdown must interrupt producer retry delay")
                .expect("producer retry worker must finish")
        );
    }

    #[test]
    fn groups_events_by_syn_report_and_does_not_forward_report_marker() {
        let events = [
            event(evdev::EventType::KEY.0, KeyCode::BTN_SOUTH.0, 1),
            event(
                evdev::EventType::SYNCHRONIZATION.0,
                evdev::SynchronizationCode::SYN_REPORT.0,
                0,
            ),
            event(evdev::EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, 123),
            event(
                evdev::EventType::ABSOLUTE.0,
                AbsoluteAxisCode::ABS_Y.0,
                -321,
            ),
            event(
                evdev::EventType::SYNCHRONIZATION.0,
                evdev::SynchronizationCode::SYN_REPORT.0,
                0,
            ),
        ];

        let frames = split_event_frames(&events);

        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].len(), 1);
        assert_eq!(frames[1].len(), 2);
        assert!(
            frames
                .iter()
                .flatten()
                .all(|event| { event.event_type() != evdev::EventType::SYNCHRONIZATION })
        );
        assert_eq!(
            split_event_frames(&[event(
                evdev::EventType::SYNCHRONIZATION.0,
                evdev::SynchronizationCode::SYN_REPORT.0,
                0,
            )]),
            vec![Vec::<InputEvent>::new()]
        );
    }

    #[test]
    fn xbox_one_s_profile_matches_go_identity_and_axes() {
        let profile = gamepad_profile();
        assert_eq!(
            profile.input_id,
            InputId::new(BusType::BUS_USB, 0x045e, 0x02ea, 0x0408)
        );
        assert_eq!(profile.trigger_max, 1023);
        assert_eq!(profile.stick_flat, 128);
        assert_eq!(profile.stick_fuzz, 16);
        let axes = gamepad_axis_setups();
        let trigger = axes
            .iter()
            .find(|axis| axis.code() == AbsoluteAxisCode::ABS_Z.0)
            .expect("left trigger axis")
            .absinfo();
        assert_eq!(trigger.maximum(), 1023);
        let stick = axes
            .iter()
            .find(|axis| axis.code() == AbsoluteAxisCode::ABS_X.0)
            .expect("left stick axis")
            .absinfo();
        assert_eq!(stick.flat(), 128);
        assert_eq!(stick.fuzz(), 16);
    }

    #[test]
    fn generic_mouse_profile_matches_go_identity() {
        assert_eq!(
            mouse_input_id(),
            InputId::new(BusType::BUS_USB, 0xdead, 0xbeef, 1)
        );
    }

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
