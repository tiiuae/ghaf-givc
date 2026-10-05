// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "givc-agent", about = "A givc agent")]
pub struct Cli {
    #[arg(long, short, env = "CONFIG")]
    pub config: PathBuf,

    #[arg(long, env = "DEBUG", default_value_t = false)]
    pub debug: bool,
}
