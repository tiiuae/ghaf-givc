// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use clap::Parser;
use givc::utils::x509::SecurityInfo;
use std::path::PathBuf;
use x509_parser::pem::parse_x509_pem;

#[derive(Debug, Parser)]
#[command(name = "givc-x509")]
struct Cli {
    cert: PathBuf,
}

fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    println!("CLI is {cli:#?}");

    let cert_file = std::fs::read(cli.cert)?;

    let (_rem, pem) = parse_x509_pem(&cert_file)?;

    let x509 = SecurityInfo::try_from(pem.contents.as_slice())?;
    println!("SI is {x509:?}");
    Ok(())
}
