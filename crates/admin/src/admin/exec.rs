use std::pin::Pin;
use std::sync::Arc;

use tokio_stream::{Stream, StreamExt};
use tonic::{Request, Response, Status, Streaming};

use crate::pb;
use crate::pb::exec::{CommandRelayRequest, CommandRequest, CommandResponse};
use crate::utils::tonic::*;
pub use pb::exec::exec_agent_server::ExecAgentServer;
pub use pb::exec::exec_server::ExecServer;

type ResponseStream = Pin<Box<dyn Stream<Item = Result<CommandResponse, Status>> + Send>>;

pub struct ExecService {}

pub struct ExecAgentService {
    inner: Arc<crate::admin::server::AdminServiceImpl>,
}

impl ExecService {
    pub fn new() -> Self {
        Self {}
    }
}

#[tonic::async_trait]
impl pb::exec::exec_server::Exec for ExecService {
    type RunCommandStream = ResponseStream;
    async fn run_command(
        &self,
        req: Request<Streaming<CommandRequest>>,
    ) -> Result<Response<Self::RunCommandStream>, Status> {
        todo!()
    }
}

impl ExecAgentService {
    pub fn new(admin: &crate::admin::server::AdminService) -> Self {
        Self {
            inner: admin.inner(),
        }
    }
}

#[tonic::async_trait]
impl pb::exec::exec_agent_server::ExecAgent for ExecAgentService {
    type RunCommandOnAgentStream = ResponseStream;
    async fn run_command_on_agent(
        &self,
        request: Request<Streaming<CommandRelayRequest>>,
    ) -> Result<Response<Self::RunCommandOnAgentStream>, Status> {
        todo!()
        /* // FIXME: postpone. I'd like to implement cli -> admin -> exec relay
        escalate(request, |req| async {
            let mut in_stream = req.into_inner();
            let Some(CommandRelayRequest{ agent, command}) = in_stream.recv().await;
            let endpoint = self.inner.agent_endpoint(&agent)?;
            let channel = endpoint.connect().await?;
            let client = pb::exec::exec_client::ExecClient::new(channel);
        })
        */
    }
}
