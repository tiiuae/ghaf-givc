use crate::pb::{self, *};
use tonic::transport::Channel;
use tonic::{metadata::MetadataValue, Code, Request, Response, Status};

type Client = pb::admin_service_client::AdminServiceClient<Channel>;

struct AdminClient {}
