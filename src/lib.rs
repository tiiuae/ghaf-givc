pub mod admin;
pub mod registry;
pub mod types;

pub mod pb {
    #![allow(dead_code)]
    #![allow(unused_imports)]
    include!(concat!(env!("OUT_DIR"), "/admin.rs"));
}

use std::{default, fmt, iter};

pub fn trace_init() {
    tracing_subscriber::fmt::init();
}
