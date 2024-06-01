use crate::pb::{self, *};
use tonic::{metadata::MetadataValue, Code, Request, Response, Status};
use tonic::transport::Channel;

type Client = pb::admin_service_client::AdminServiceClient<Channel>;

struct AdminClient {

}
