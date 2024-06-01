use crate::pb::{self, *};
use tonic::{metadata::MetadataValue, Code, Request, Response, Status};
use tonic::transport::Channel;

type Client = pb::systemd::unit_control_service_client::UnitControlServiceClient<Channel>;

struct SystemDClient {

}
