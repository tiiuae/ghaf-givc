use anyhow::Context;
use async_stream::{stream, try_stream};
use std::future::Future;
use std::pin::Pin;
use tokio_stream::{Stream, StreamExt};
use tonic::Request;
use tonic::transport::Channel;
use tracing::{debug, warn};

use crate::endpoint::EndpointConfig;
use crate::stream::check_trailers;
use givc_common::pb::exec::command_request::Command;
use givc_common::pb::exec::command_response::Event;
use givc_common::pb::exec::{CommandIo, CommandRequest, StartCommand};

type CommandIO = CommandIo; // Just for sense of prettyness
type Client = givc_common::pb::exec::exec_client::ExecClient<Channel>;

/// `ExecClient` struct for interacting with the gRPC server
pub struct ExecClient {
    client: Client,
}

pub enum CommandOutput {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
}

impl ExecClient {
    /// Connects to the gRPC server at the specified address
    /// # Errors
    /// Raise error if unable to connect
    pub async fn connect(endpoint: EndpointConfig) -> anyhow::Result<Self> {
        let channel = endpoint.connect().await?;
        let client = Client::new(channel);
        Ok(Self { client })
    }

    /// Starts a command with bidirectional streaming
    /// # Errors
    /// Raise error if unable to connect
    pub async fn start_command_stream(
        &mut self,
        command: String,
        arguments: Vec<String>,
        working_directory: Option<String>,
        env_vars: Option<std::collections::HashMap<String, String>>,
        mut stdin: impl Stream<Item = Vec<u8>> + std::marker::Unpin + Send + Sync + 'static,
        role: Option<String>,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<CommandOutput>> + Send + Sync>>>
    {
        let start_command = StartCommand {
            command,
            arguments,
            working_directory,
            env_vars: env_vars.unwrap_or_default(),
            stdin: Some(vec![]),
            role,
        };

        // Create a streaming request
        let request_stream = stream! {
            // Send the StartCommand
            yield CommandRequest {
                command: Some(Command::Start(start_command)),
            };

            while let Some(input) = stdin.next().await {
                yield CommandRequest {
                    command: Some(Command::Stdin(CommandIo { payload: input, eof: false }))
                };
            }

            yield CommandRequest { command: Some(Command::Stdin(CommandIo { payload: vec![], eof: true })) };
        };

        // Open the request stream and capture responses
        let mut response = self
            .client
            .run_command(Request::new(request_stream))
            .await?
            .into_inner();

        let response_stream = try_stream! {
            while let Some(response) = response.message().await? {
                match response.event {
                    Some(Event::Stdout(CommandIO { payload, .. })) => {
                        debug!(
                            "Event::Stdout {len} bytes: {out}",
                            len = payload.len(),
                            out = String::from_utf8_lossy(&payload)
                        );
                        yield CommandOutput::Stdout(payload);
                    }
                    Some(Event::Stderr(CommandIO { payload, .. })) => {
                        debug!(
                            "Event::Stderr {len} bytes: {out}",
                            len = payload.len(),
                            out = String::from_utf8_lossy(&payload)
                        );
                        yield CommandOutput::Stderr(payload);
                    }
                    Some(Event::Started(started)) => {
                        debug!("Process started with PID: {}", started.pid);
                    }
                    Some(Event::Finished(finished)) => {
                        debug!("Process finished with exit code: {}", finished.return_code);
                    }
                    None => {
                        warn!("Received empty response");
                    }
                }
            }

            check_trailers(response)
                .await
                .context("While check trailers in exec.rs")?;
        };

        Ok(Box::pin(response_stream))
    }

    /// Starts a subprocess on the server with the given command and arguments
    /// # Errors
    /// Raise error if program unable to execute, or on gRPC IO errors
    #[allow(clippy::too_many_arguments)]
    pub async fn start_command<SOA, SEA>(
        &mut self,
        command: String,
        arguments: Vec<String>,
        working_directory: Option<String>,
        env_vars: Option<std::collections::HashMap<String, String>>,
        stdin: Option<Vec<u8>>,
        role: Option<String>,
        mut stdout_fn: impl FnMut(Vec<u8>, bool) -> SOA,
        mut stderr_fn: impl FnMut(Vec<u8>, bool) -> SEA,
    ) -> anyhow::Result<i32>
    where
        SOA: Future<Output = ()>,
        SEA: Future<Output = ()>,
    {
        let has_stdin = stdin.is_some();
        let start_command = StartCommand {
            command,
            arguments,
            working_directory,
            env_vars: env_vars.unwrap_or_default(),
            stdin,
            role,
        };

        // Create a streaming request
        let request_stream = stream! {
            // Send the StartCommand
            yield CommandRequest {
                command: Some(Command::Start(start_command)),
            };
            if has_stdin {
                yield CommandRequest {
                    command: Some(Command::Stdin(CommandIO { payload: vec![], eof: true })),
                };
            }
        };

        // Open the request stream and capture responses
        let mut response = self
            .client
            .run_command(Request::new(request_stream))
            .await?
            .into_inner();

        let mut return_code = -1;

        debug!("Subprocess started. Waiting for output...");

        // Process the server's responses
        while let Some(response) = response.message().await? {
            match response.event {
                Some(Event::Stdout(CommandIO { payload, eof })) => {
                    debug!(
                        "Event::Stdout {} bytes: {out}",
                        payload.len(),
                        out = String::from_utf8_lossy(&payload)
                    );
                    stdout_fn(payload, eof).await;
                }
                Some(Event::Stderr(CommandIO { payload, eof })) => {
                    debug!(
                        "Event::Stderr {} bytes: {out}",
                        payload.len(),
                        out = String::from_utf8_lossy(&payload)
                    );
                    stderr_fn(payload, eof).await;
                }
                Some(Event::Started(started)) => {
                    debug!("Process started with PID: {}", started.pid);
                }
                Some(Event::Finished(finished)) => {
                    debug!("Process finished with exit code: {}", finished.return_code);
                    return_code = finished.return_code;
                    break;
                }
                None => {
                    warn!("Received empty response");
                }
            }
        }

        check_trailers(response)
            .await
            .context("While check trailers in exec.rs")?;

        Ok(return_code)
    }

    /// Starts a subprocess on the server with the given command and arguments
    /// # Errors
    /// Raise error if program unable to execute, or on gRPC IO errors
    pub async fn get_program_output(
        &mut self,
        command: String,
        arguments: Vec<String>,
        working_directory: Option<String>,
        env_vars: Option<std::collections::HashMap<String, String>>,
        stdin: Option<Vec<u8>>,
        role: Option<String>,
    ) -> anyhow::Result<(Vec<u8>, Vec<u8>, i32)> {
        // Buffers for stdout and stderr
        let mut stdout_buffer = Vec::new();
        let mut stderr_buffer = Vec::new();
        let rc = self
            .start_command(
                command,
                arguments,
                working_directory,
                env_vars,
                stdin,
                role,
                |payload, _eof| {
                    stdout_buffer.extend(&payload);
                    std::future::ready(())
                },
                |payload, _eof| {
                    stderr_buffer.extend(&payload);
                    std::future::ready(())
                },
            )
            .await?;
        Ok((stdout_buffer, stderr_buffer, rc))
    }
}
