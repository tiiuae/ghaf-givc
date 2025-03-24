use tokio::io::{stdin, AsyncReadExt};
use tonic::transport::Channel;
use tonic::Request;
use tracing::{info, warn};

use crate::endpoint::EndpointConfig;
use givc_common::pb::exec::command_request::Command;
use givc_common::pb::exec::command_response::Event;
use givc_common::pb::exec::{CommandIo, CommandRequest, CommandResponse, StartCommand};

type CommandIO = CommandIo; // Just for sense of prettyness
type Client = givc_common::pb::exec::exec_client::ExecClient<Channel>;

/// ExecClient struct for interacting with the gRPC server
pub struct ExecClient {
    client: Client,
}

impl ExecClient {
    /// Connects to the gRPC server at the specified address
    pub async fn connect(endpoint: EndpointConfig) -> anyhow::Result<Self> {
        let channel = endpoint.connect().await?;
        let client = Client::new(channel);
        Ok(Self { client })
    }

    /// Starts a subprocess on the server with the given command and arguments
    pub async fn start_command(
        &mut self,
        command: String,
        arguments: Vec<String>,
        working_directory: Option<String>,
        env_vars: Option<std::collections::HashMap<String, String>>,
        stdin: Option<Vec<u8>>,
        role: Option<String>,
    ) -> anyhow::Result<(Vec<u8>, Vec<u8>, i32)> {
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

        // Buffers for stdout and stderr
        let mut stdout_buffer = Vec::new();
        let mut stderr_buffer = Vec::new();
        let mut return_code = -1;

        info!("Subprocess started. Waiting for output...");

        // Process the server's responses
        while let Some(response) = response.message().await? {
            match response.event {
                Some(Event::Stdout(CommandIO { payload })) => {
                    info!("Event::Stdout {} bytes", payload.len());
                    stdout_buffer.extend(payload);
                }
                Some(Event::Stderr(CommandIO { payload })) => {
                    info!("Event::Stderr {} bytes", payload.len());
                    stderr_buffer.extend(payload);
                }
                Some(Event::Started(started)) => {
                    info!("Process started with PID: {}", started.pid);
                }
                Some(Event::Finished(finished)) => {
                    info!("Process finished with exit code: {}", finished.return_code);
                    return_code = finished.return_code;
                    break;
                }
                None => {
                    warn!("Received empty response");
                }
            }
        }

        Ok((stdout_buffer, stderr_buffer, return_code))
    }
}
