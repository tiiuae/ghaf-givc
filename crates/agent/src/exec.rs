// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;
use std::fs::File;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use anyhow::{Result, bail};
use givc_common::pb;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

pub use pb::exec::exec_server::ExecServer as ExecServiceServer;

#[derive(Debug, Clone)]
pub struct ExecService {
    allowed_commands: HashSet<&'static str>,
}

impl ExecService {
    #[must_use]
    pub fn new() -> Self {
        Self {
            allowed_commands: HashSet::from(["ota-update", "uptime"]),
        }
    }

    fn validate_command(&self, cmd: &str) -> Result<()> {
        if self.allowed_commands.contains(cmd) {
            Ok(())
        } else {
            bail!("access denied: command \"{cmd}\" is not allowed")
        }
    }
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

        self.validate_command(&start.command).map_err(map_err)?;

        let (tx, rx) = mpsc::channel(32);
        let mut cmd = Command::new(&start.command);
        cmd.args(&start.arguments);
        if let Some(dir) = &start.working_directory {
            cmd.current_dir(dir);
        }
        cmd.envs(&start.env_vars);

        let mut stdin_pipe = None;
        if start.stdin.is_some() {
            cmd.stdin(Stdio::piped());
        } else {
            cmd.stdin(File::open("/dev/null").map_err(|err| Status::internal(err.to_string()))?);
        }
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|err| Status::internal(err.to_string()))?;
        let pid = child.id() as i32;

        if let Some(stdin) = child.stdin.take() {
            stdin_pipe = Some(Arc::new(Mutex::new(stdin)));
        }

        let _ = tx.blocking_send(Ok(pb::exec::CommandResponse {
            event: Some(pb::exec::command_response::Event::Started(
                pb::exec::StartedEvent { pid },
            )),
        }));

        if let Some(mut stdout) = child.stdout.take() {
            let tx = tx.clone();
            std::thread::spawn(move || {
                stream_reader(&mut stdout, tx, |data| pb::exec::CommandResponse {
                    event: Some(pb::exec::command_response::Event::Stdout(
                        pb::exec::CommandIo { payload: data },
                    )),
                });
            });
        }

        if let Some(mut stderr) = child.stderr.take() {
            let tx = tx.clone();
            std::thread::spawn(move || {
                stream_reader(&mut stderr, tx, |data| pb::exec::CommandResponse {
                    event: Some(pb::exec::command_response::Event::Stderr(
                        pb::exec::CommandIo { payload: data },
                    )),
                });
            });
        }

        if let Some(stdin) = stdin_pipe.clone() {
            if let Some(initial) = start.stdin {
                if !initial.is_empty() {
                    let mut guard = stdin.lock().expect("stdin poisoned");
                    guard
                        .write_all(&initial)
                        .map_err(|err| Status::internal(err.to_string()))?;
                }
            }
            let signal_pid = pid;
            let tx = tx.clone();
            tokio::spawn(async move {
                while let Some(msg) = inbound.next().await {
                    let Ok(msg) = msg else {
                        break;
                    };
                    match msg.command {
                        Some(pb::exec::command_request::Command::Stdin(stdin_msg)) => {
                            if let Ok(mut guard) = stdin.lock() {
                                let _ = guard.write_all(&stdin_msg.payload);
                            }
                        }
                        Some(pb::exec::command_request::Command::Signal(sig)) => {
                            let _ = unsafe { libc::kill(signal_pid, sig.signal) };
                        }
                        Some(pb::exec::command_request::Command::Start(_)) | None => {}
                    }
                }
                drop(tx);
            });
        }

        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut child = child;
            let status = child.wait();
            let code = match status {
                Ok(status) => status.code().unwrap_or_default(),
                Err(_) => 1,
            };
            let _ = tx.blocking_send(Ok(pb::exec::CommandResponse {
                event: Some(pb::exec::command_response::Event::Finished(
                    pb::exec::FinishedEvent { return_code: code },
                )),
            }));
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }
}

fn stream_reader<R, F>(
    reader: &mut R,
    tx: mpsc::Sender<Result<pb::exec::CommandResponse, Status>>,
    make_response: F,
) where
    R: Read,
    F: Fn(Vec<u8>) -> pb::exec::CommandResponse,
{
    let mut buffer = [0_u8; 1024];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => {
                let _ = tx.blocking_send(Ok(make_response(buffer[..n].to_vec())));
            }
            Err(_) => break,
        }
    }
}

fn map_err(err: anyhow::Error) -> Status {
    Status::permission_denied(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_command_allowlist() {
        let svc = ExecService::new();
        assert!(svc.validate_command("uptime").is_ok());
        assert!(svc.validate_command("nope").is_err());
    }
}
