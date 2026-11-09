// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use anyhow::{Result, bail};
use givc_common::pb;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

pub use pb::exec::exec_server::ExecServer as ExecServiceServer;

#[derive(Debug, Clone)]
pub struct ExecService {
    allowed_commands: HashSet<String>,
}

impl ExecService {
    #[must_use]
    pub fn new() -> Self {
        Self {
            allowed_commands: HashSet::from(["ota-update".to_owned(), "uptime".to_owned()]),
        }
    }

    fn validate_command(&self, cmd: &str) -> Result<()> {
        if self.allowed_commands.contains(cmd) {
            Ok(())
        } else {
            bail!("access denied: command \"{cmd}\" is not allowed")
        }
    }

    fn resolve_command(&self, command: &str) -> Result<PathBuf> {
        self.validate_command(command)?;

        let path = Path::new(command);
        if path.is_absolute() {
            if is_executable(path) {
                return Ok(path.to_owned());
            }
            bail!("allowed executable does not exist: {command}");
        }

        let search_path = std::env::var_os("PATH").unwrap_or_default();
        for directory in std::env::split_paths(&search_path) {
            let candidate = directory.join(command);
            if is_executable(&candidate) {
                return Ok(candidate);
            }
        }

        bail!("allowed executable was not found in server PATH: {command}")
    }
}

fn is_executable(path: &Path) -> bool {
    std::fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[tonic::async_trait]
impl pb::exec::exec_server::Exec for ExecService {
    type RunCommandStream = ReceiverStream<Result<pb::exec::CommandResponse, Status>>;

    async fn run_command(
        &self,
        request: Request<tonic::Streaming<pb::exec::CommandRequest>>,
    ) -> Result<Response<Self::RunCommandStream>, Status> {
        let mut inbound = request.into_inner();
        let first = inbound
            .message()
            .await
            .map_err(|err| Status::internal(err.to_string()))?
            .ok_or_else(|| Status::invalid_argument("failed to receive start command"))?;

        let start = match first.command {
            Some(pb::exec::command_request::Command::Start(start)) => start,
            _ => {
                return Err(Status::invalid_argument(
                    "expected StartCommand, got something else",
                ));
            }
        };

        let executable = self.resolve_command(&start.command).map_err(map_err)?;

        let (tx, rx) = mpsc::channel(32);
        let mut cmd = Command::new(executable);
        cmd.args(&start.arguments);
        if let Some(dir) = &start.working_directory {
            cmd.current_dir(dir);
        }
        cmd.env_clear().envs(&start.env_vars);

        let mut stdin_pipe = None;
        if start.stdin.is_some() {
            cmd.stdin(Stdio::piped());
        } else {
            cmd.stdin(Stdio::null());
        }
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|err| Status::internal(err.to_string()))?;
        let pid = child.id().unwrap_or_default() as i32;

        if let Some(stdin) = child.stdin.take() {
            stdin_pipe = Some(Arc::new(Mutex::new(stdin)));
        }

        let _ = tx
            .send(Ok(pb::exec::CommandResponse {
                event: Some(pb::exec::command_response::Event::Started(
                    pb::exec::StartedEvent { pid },
                )),
            }))
            .await;

        let mut reader_tasks = Vec::new();
        if let Some(mut stdout) = child.stdout.take() {
            let tx = tx.clone();
            reader_tasks.push(tokio::spawn(async move {
                stream_reader(&mut stdout, tx, |data| pb::exec::CommandResponse {
                    event: Some(pb::exec::command_response::Event::Stdout(
                        pb::exec::CommandIo { payload: data },
                    )),
                })
                .await;
            }));
        }

        if let Some(mut stderr) = child.stderr.take() {
            let tx = tx.clone();
            reader_tasks.push(tokio::spawn(async move {
                stream_reader(&mut stderr, tx, |data| pb::exec::CommandResponse {
                    event: Some(pb::exec::command_response::Event::Stderr(
                        pb::exec::CommandIo { payload: data },
                    )),
                })
                .await;
            }));
        }

        if let (Some(stdin), Some(initial)) = (stdin_pipe.as_ref(), start.stdin.as_ref()) {
            if !initial.is_empty() {
                let mut guard = stdin.lock().await;
                guard
                    .write_all(initial)
                    .await
                    .map_err(|err| Status::internal(err.to_string()))?;
            }
        }

        let input_stdin = stdin_pipe;
        let signal_pid = pid;
        let (input_done_tx, input_done_rx) = tokio::sync::oneshot::channel();
        let input_task = tokio::spawn(async move {
            tokio::pin!(input_done_rx);
            loop {
                tokio::select! {
                    _ = &mut input_done_rx => break,
                    msg = inbound.next() => {
                        let Some(Ok(msg)) = msg else { break };
                        match msg.command {
                            Some(pb::exec::command_request::Command::Stdin(stdin_msg)) => {
                                if let Some(stdin) = &input_stdin {
                                    let mut guard = stdin.lock().await;
                                    let _ = guard.write_all(&stdin_msg.payload).await;
                                }
                            }
                            Some(pb::exec::command_request::Command::Signal(sig)) => {
                                if let Ok(signal) = nix::sys::signal::Signal::try_from(sig.signal) {
                                    let _ = nix::sys::signal::kill(
                                        nix::unistd::Pid::from_raw(signal_pid),
                                        signal,
                                    );
                                }
                            }
                            Some(pb::exec::command_request::Command::Start(_)) | None => {}
                        }
                    }
                }
            }
        });

        let tx = tx.clone();
        tokio::spawn(async move {
            let status = child.wait().await;
            let _ = input_done_tx.send(());
            let _ = input_task.await;
            for reader_task in reader_tasks {
                let _ = reader_task.await;
            }
            let code = match status {
                Ok(status) => status.code().unwrap_or(-1),
                Err(_) => 1,
            };
            let _ = tx
                .send(Ok(pb::exec::CommandResponse {
                    event: Some(pb::exec::command_response::Event::Finished(
                        pb::exec::FinishedEvent { return_code: code },
                    )),
                }))
                .await;
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }
}

async fn stream_reader<R, F>(
    reader: &mut R,
    tx: mpsc::Sender<Result<pb::exec::CommandResponse, Status>>,
    make_response: F,
) where
    R: AsyncRead + Unpin,
    F: Fn(Vec<u8>) -> pb::exec::CommandResponse,
{
    let mut buffer = [0_u8; 1024];
    loop {
        match reader.read(&mut buffer).await {
            Ok(0) => break,
            Ok(n) => {
                let _ = tx.send(Ok(make_response(buffer[..n].to_vec()))).await;
            }
            Err(_) => break,
        }
    }
}

fn map_err(err: anyhow::Error) -> Status {
    // Command allowlist failures must remain permission errors.
    Status::permission_denied(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::os::unix::fs::PermissionsExt;
    use tokio::net::TcpListener;
    use tokio_stream::wrappers::{ReceiverStream, TcpListenerStream};

    async fn start_test_command(
        allowed_commands: Vec<String>,
        command: &str,
        arguments: Vec<String>,
        env_vars: HashMap<String, String>,
        stdin: Option<Vec<u8>>,
    ) -> (
        tokio::task::JoinHandle<()>,
        mpsc::Sender<pb::exec::CommandRequest>,
        tonic::Streaming<pb::exec::CommandResponse>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let service = ExecService {
            allowed_commands: allowed_commands.into_iter().collect(),
        };
        let server = tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(ExecServiceServer::new(service))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .unwrap();
        });
        let mut client = pb::exec::exec_client::ExecClient::connect(format!("http://{address}"))
            .await
            .unwrap();
        let (request_tx, request_rx) = mpsc::channel(8);
        request_tx
            .send(pb::exec::CommandRequest {
                command: Some(pb::exec::command_request::Command::Start(
                    pb::exec::StartCommand {
                        command: command.to_owned(),
                        arguments,
                        working_directory: None,
                        env_vars,
                        stdin,
                        role: None,
                    },
                )),
            })
            .await
            .unwrap();
        let response = client
            .run_command(ReceiverStream::new(request_rx))
            .await
            .unwrap()
            .into_inner();
        (server, request_tx, response)
    }

    fn system_program(name: &str) -> String {
        std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|directory| directory.join(name))
            .find(|path| path.is_file())
            .unwrap_or_else(|| panic!("{name} not found in test PATH"))
            .to_string_lossy()
            .into_owned()
    }

    async fn collect_events(
        response: &mut tonic::Streaming<pb::exec::CommandResponse>,
    ) -> Vec<pb::exec::command_response::Event> {
        let mut events = Vec::new();
        while let Some(response) = response.message().await.unwrap() {
            if let Some(event) = response.event {
                events.push(event);
            }
        }
        events
    }

    #[test]
    fn validate_command_allowlist() {
        let svc = ExecService::new();
        assert!(svc.validate_command("uptime").is_ok());
        assert!(svc.validate_command("nope").is_err());
    }

    #[tokio::test]
    async fn caller_path_cannot_shadow_an_allowlisted_command() {
        let directory = std::env::temp_dir().join(format!("givc-exec-path-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let shadow = directory.join("uptime");
        std::fs::write(&shadow, "#!/bin/sh\nprintf INJECTED\n").unwrap();
        std::fs::set_permissions(&shadow, std::fs::Permissions::from_mode(0o755)).unwrap();

        let (server, _request_tx, mut response) = start_test_command(
            vec!["uptime".to_owned()],
            "uptime",
            vec![],
            HashMap::from([("PATH".to_owned(), directory.to_string_lossy().into_owned())]),
            None,
        )
        .await;
        let events = collect_events(&mut response).await;
        server.abort();
        std::fs::remove_dir_all(directory).unwrap();

        let output = events
            .iter()
            .filter_map(|event| match event {
                pb::exec::command_response::Event::Stdout(output) => Some(&output.payload),
                _ => None,
            })
            .flatten()
            .copied()
            .collect::<Vec<_>>();
        assert!(!output.windows(8).any(|window| window == b"INJECTED"));
    }

    #[tokio::test]
    async fn child_environment_contains_only_requested_variables() {
        let (server, _request_tx, mut response) = start_test_command(
            vec!["/usr/bin/env".to_owned()],
            "/usr/bin/env",
            vec![],
            HashMap::from([("GIVC_EXEC_TEST".to_owned(), "present".to_owned())]),
            None,
        )
        .await;
        let events = collect_events(&mut response).await;
        server.abort();

        let output = events
            .iter()
            .filter_map(|event| match event {
                pb::exec::command_response::Event::Stdout(output) => {
                    Some(output.payload.as_slice())
                }
                _ => None,
            })
            .flatten()
            .copied()
            .collect::<Vec<_>>();
        assert_eq!(output, b"GIVC_EXEC_TEST=present\n");
    }

    #[tokio::test]
    async fn initial_stdin_is_forwarded_and_eof_closes_child_stdin() {
        let cat = system_program("cat");
        let (server, request_tx, mut response) = start_test_command(
            vec![cat.clone()],
            &cat,
            vec![],
            HashMap::new(),
            Some(b"stdin-payload".to_vec()),
        )
        .await;
        drop(request_tx);
        let events = collect_events(&mut response).await;
        server.abort();

        let output = events
            .iter()
            .filter_map(|event| match event {
                pb::exec::command_response::Event::Stdout(output) => {
                    Some(output.payload.as_slice())
                }
                _ => None,
            })
            .flatten()
            .copied()
            .collect::<Vec<_>>();
        assert_eq!(output, b"stdin-payload");
        assert!(matches!(
            events.last(),
            Some(pb::exec::command_response::Event::Finished(_))
        ));
    }

    #[tokio::test]
    async fn no_stdin_starts_child_with_closed_standard_input() {
        let cat = system_program("cat");
        let (server, _request_tx, mut response) =
            start_test_command(vec![cat.clone()], &cat, vec![], HashMap::new(), None).await;
        let events = collect_events(&mut response).await;
        server.abort();

        assert!(
            !events
                .iter()
                .any(|event| matches!(event, pb::exec::command_response::Event::Stdout(_)))
        );
        assert!(matches!(
            events.last(),
            Some(pb::exec::command_response::Event::Finished(_))
        ));
    }

    #[tokio::test]
    async fn signal_is_forwarded_without_stdin_and_reports_signal_exit() {
        let sleep = system_program("sleep");
        let (server, request_tx, mut response) = start_test_command(
            vec![sleep.clone()],
            &sleep,
            vec!["30".to_owned()],
            HashMap::new(),
            None,
        )
        .await;
        let started = response.message().await.unwrap().unwrap();
        let pid = match started.event.unwrap() {
            pb::exec::command_response::Event::Started(started) => started.pid,
            event => panic!("expected Started, got {event:?}"),
        };
        request_tx
            .send(pb::exec::CommandRequest {
                command: Some(pb::exec::command_request::Command::Signal(
                    pb::exec::SignalCommand { signal: 15 },
                )),
            })
            .await
            .unwrap();
        let events = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            collect_events(&mut response),
        )
        .await;
        server.abort();

        assert!(pid > 0);
        let events = match events {
            Ok(events) => events,
            Err(_) => {
                let _ = nix::sys::signal::kill(
                    nix::unistd::Pid::from_raw(pid),
                    nix::sys::signal::Signal::SIGKILL,
                );
                panic!("signal was not forwarded to child process");
            }
        };
        let Some(pb::exec::command_response::Event::Finished(finished)) = events.last() else {
            panic!("expected Finished event");
        };
        assert_eq!(finished.return_code, -1);
    }

    #[tokio::test]
    async fn output_events_precede_finished_event() {
        let shell = system_program("sh");
        let (server, _request_tx, mut response) = start_test_command(
            vec![shell.clone()],
            &shell,
            vec!["-c".to_owned(), "printf output-before-finished".to_owned()],
            HashMap::new(),
            None,
        )
        .await;
        let events = collect_events(&mut response).await;
        server.abort();

        assert!(matches!(
            events.first(),
            Some(pb::exec::command_response::Event::Started(_))
        ));
        assert!(matches!(
            events.get(1),
            Some(pb::exec::command_response::Event::Stdout(_))
        ));
        assert!(matches!(
            events.last(),
            Some(pb::exec::command_response::Event::Finished(_))
        ));
    }

    #[tokio::test]
    async fn client_disconnect_closes_piped_stdin_and_finishes_child() {
        let cat = system_program("cat");
        let (server, request_tx, mut response) = start_test_command(
            vec![cat.clone()],
            &cat,
            vec![],
            HashMap::new(),
            Some(Vec::new()),
        )
        .await;
        let _started = response.message().await.unwrap().unwrap();
        drop(request_tx);
        let events = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            collect_events(&mut response),
        )
        .await
        .expect("child should finish after client closes input");
        server.abort();

        assert!(matches!(
            events.last(),
            Some(pb::exec::command_response::Event::Finished(_))
        ));
    }
}
