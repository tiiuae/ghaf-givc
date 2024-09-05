pub mod address;
pub mod query;
pub mod types;

pub mod pb {
    pub mod admin {
        tonic::include_proto!("admin");
    }
    pub mod systemd {
        tonic::include_proto!("systemd");
    }
    pub mod reflection {
        pub const ADMIN_DESCRIPTOR: &[u8] = tonic::include_file_descriptor_set!("admin_descriptor");
        pub const SYSTEMD_DESCRIPTOR: &[u8] =
            tonic::include_file_descriptor_set!("systemd_descriptor");
    }
    // Re-export to keep current code untouched
    pub use crate::pb::admin::*;
}
