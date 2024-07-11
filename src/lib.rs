pub mod admin;
pub mod systemd_api;
pub mod utils;

pub mod pb {
    // Re-export to keep current code untouched
    pub use givc_common::pb::*;
}
pub use givc_common::types;
pub use givc_client::endpoint;

pub fn trace_init() {
    tracing_subscriber::fmt::init();
}
