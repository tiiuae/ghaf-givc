use anyhow::Context;
use std::future::Future;
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

impl ExecClient {
    /// Connects to the gRPC server at the specified address
    /// # Errors
    /// Raise error if unable to connect
    pub async fn connect(endpoint: EndpointConfig) -> anyhow::Result<Self> {
        let channel = endpoint.connect().await?;
        let client = Client::new(channel);
        Ok(Self { client })
    }

    /// Starts a subprocess on the server with the given command and arguments
    /// # Errors
    /// Raise error if program unable to execute, or on gRPC IO errors
    #[allow(clippy::too_many_arguments)]
    pub async fn start_command<SO, SE, SOA, SEA>(
        &mut self,
        command: String,
        arguments: Vec<String>,
        working_directory: Option<String>,
        env_vars: Option<std::collections::HashMap<String, String>>,
        stdin: Option<Vec<u8>>,
        role: Option<String>,
        mut stdout_fn: SO,
        mut stderr_fn: SE,
    ) -> anyhow::Result<i32>
    where
        SO: FnMut(Vec<u8>) -> SOA,
        SE: FnMut(Vec<u8>) -> SEA,
        SOA: Future<Output = ()>,
        SEA: Future<Output = ()>,
    {
        let start_command = StartCommand {
            command,
            arguments,
            working_directory,
            env_vars: env_vars.unwrap_or_default(),
            stdin,
            role,
        };

        // Create a streaming request
        let request_stream = tokio_stream::once(
            // Send the StartCommand
            CommandRequest {
                command: Some(Command::Start(start_command)),
            },
        );

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
                Some(Event::Stdout(CommandIO { payload })) => {
                    debug!(
                        "Event::Stdout {} bytes: {out}",
                        payload.len(),
                        out = String::from_utf8_lossy(&payload)
                    );
                    stdout_fn(payload).await;
                }
                Some(Event::Stderr(CommandIO { payload })) => {
                    debug!(
                        "Event::Stderr {} bytes: {out}",
                        payload.len(),
                        out = String::from_utf8_lossy(&payload)
                    );
                    stderr_fn(payload).await;
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
                |payload| {
                    stdout_buffer.extend(&payload);
                    std::future::ready(())
                },
                |payload| {
                    stderr_buffer.extend(&payload);
                    std::future::ready(())
                },
            )
            .await?;
        Ok((stdout_buffer, stderr_buffer, rc))
    }
}
