// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

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
    pub mod policyadmin {
        tonic::include_proto!("policyadmin");
    }
    pub mod stats {
        tonic::include_proto!("stats");
    }
    pub mod notify {
        tonic::include_proto!("notify");
    }
    pub mod ctap {
        tonic::include_proto!("ctap");
    }
    pub mod hwid {
        tonic::include_proto!("hwid");
    }
    pub mod socketproxy {
        tonic::include_proto!("socketproxy");
    }
    pub mod eventproxy {
        tonic::include_proto!("eventproxy");
    }
    pub mod wifi {
        tonic::include_proto!("wifimanager");
    }
    pub mod reflection {
        pub const ADMIN_DESCRIPTOR: &[u8] = tonic::include_file_descriptor_set!("admin_descriptor");
        pub const EXEC_DESCRIPTOR: &[u8] = tonic::include_file_descriptor_set!("exec_descriptor");
        pub const CTAP_DESCRIPTOR: &[u8] = tonic::include_file_descriptor_set!("ctap_descriptor");
        pub const POLICYADMIN_DESCRIPTOR: &[u8] =
            tonic::include_file_descriptor_set!("policyadmin_descriptor");
        pub const SOCKET_DESCRIPTOR: &[u8] =
            tonic::include_file_descriptor_set!("socket_descriptor");
        pub const EVENT_DESCRIPTOR: &[u8] = tonic::include_file_descriptor_set!("event_descriptor");
        pub const WIFI_DESCRIPTOR: &[u8] = tonic::include_file_descriptor_set!("wifi_descriptor");
        pub const NOTIFY_DESCRIPTOR: &[u8] =
            tonic::include_file_descriptor_set!("notify_descriptor");
        pub const LOCALE_DESCRIPTOR: &[u8] =
            tonic::include_file_descriptor_set!("locale_descriptor");
        pub const HWID_DESCRIPTOR: &[u8] = tonic::include_file_descriptor_set!("hwid_descriptor");
        pub const SYSTEMD_DESCRIPTOR: &[u8] =
            tonic::include_file_descriptor_set!("systemd_descriptor");
    }
    // Re-export to keep current code untouched
    pub use crate::pb::admin::*;
}
