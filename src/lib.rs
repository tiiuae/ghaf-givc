pub mod admin;
pub mod endpoint;
pub mod systemd_api;
pub mod types;

pub mod pb {
    pub mod admin {
        tonic::include_proto!("admin");
    }
    pub mod systemd {
        tonic::include_proto!("systemd");
    }
    // Re-export to keep current code untouched
    pub use crate::pb::admin::*;
}

use std::{default, fmt, iter};

pub fn trace_init() {
    tracing_subscriber::fmt::init();
}
