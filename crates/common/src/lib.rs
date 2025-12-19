pub mod address;
pub mod query;
pub mod types;

pub mod pb {
    // Suppress clippy on generated code
    #![allow(clippy::all)]
    #![allow(clippy::missing_errors_doc)]
    #![allow(clippy::default_trait_access)]
    #![allow(clippy::too_many_lines)]
    #![allow(clippy::manual_string_new)]
    #![allow(clippy::must_use_candidate)]
    #![allow(clippy::similar_names)]
    #![allow(clippy::doc_markdown)]
    pub mod admin {
        use serde::{Deserialize, Serialize};
        tonic::include_proto!("admin");
    }
    pub mod exec {
        tonic::include_proto!("exec");
    }
    #[allow(clippy::doc_markdown)]
    pub mod locale {
        tonic::include_proto!("locale");
    }
    pub mod systemd {
        tonic::include_proto!("systemd");
    }
    pub mod policyagent {
        tonic::include_proto!("policyagent");
    }
    pub mod stats {
        tonic::include_proto!("stats");
    }
    pub mod notify {
        tonic::include_proto!("notify");
    }
    pub mod reflection {
        pub const ADMIN_DESCRIPTOR: &[u8] = tonic::include_file_descriptor_set!("admin_descriptor");
        pub const SYSTEMD_DESCRIPTOR: &[u8] =
            tonic::include_file_descriptor_set!("systemd_descriptor");
    }
    // Re-export to keep current code untouched
    pub use crate::pb::admin::*;
}
