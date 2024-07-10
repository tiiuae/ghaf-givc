use clap::Parser;
use std::path::PathBuf;
use x509_parser::pem::parse_x509_pem;
use x509_parser::prelude::*;

#[derive(Debug, Parser)]
#[command(name = "givc-x509")]
struct Cli {
    cert: PathBuf,
}

fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    println!("CLI is {:#?}", cli);

    let cert_file = std::fs::read(cli.cert)?;

    let (_rem, pem) = parse_x509_pem(&cert_file)?;

    let x509 = parse_x509_certificate(&pem.contents)?;
    println!("X509: {:#?}", x509);
    Ok(())
}
