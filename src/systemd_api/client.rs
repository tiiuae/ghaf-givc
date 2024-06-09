use crate::pb::{self, *};
use tonic::transport::Channel;
use tonic::{metadata::MetadataValue, Code, Request, Response, Status};

type Client = pb::systemd::unit_control_service_client::UnitControlServiceClient<Channel>;

struct SystemDClient {}
