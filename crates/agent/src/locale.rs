// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::process::Command;

use anyhow::{Context, Result, bail};
use givc_common::pb;
use regex::Regex;
use tonic::{Request, Response, Status};

pub use pb::locale::locale_client_server::LocaleClientServer;

#[derive(Debug, Default, Clone)]
pub struct LocaleServer;

impl LocaleServer {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    fn set_locale(&self, assignments: &[pb::locale::LocaleAssignment]) -> Result<()> {
        if assignments.is_empty() {
            bail!("no locale assignments provided")
        }

        let args = build_locale_args(assignments)?;
        run_command(
            "localectl",
            ["set-locale"]
                .into_iter()
                .chain(args.iter().map(String::as_str)),
        )
        .context("failed to set locale")?;

        let systemctl_args = if is_root() {
            vec!["set-environment".to_owned()]
        } else {
            vec!["--user".to_owned(), "set-environment".to_owned()]
        };
        let _ = run_command(
            "systemctl",
            systemctl_args
                .iter()
                .map(String::as_str)
                .chain(args.iter().map(String::as_str)),
        );

        Ok(())
    }

    fn set_timezone(&self, timezone: &str) -> Result<()> {
        if !validate_timezone(timezone) {
            bail!("invalid timezone")
        }

        let _ = run_command("timedatectl", ["set-timezone", timezone]);
        Ok(())
    }
}

#[tonic::async_trait]
impl pb::locale::locale_client_server::LocaleClient for LocaleServer {
    async fn locale_set(
        &self,
        request: Request<pb::locale::LocaleMessage>,
    ) -> Result<Response<pb::locale::Empty>, Status> {
        let req = request.into_inner();
        self.set_locale(&req.assignments).map_err(map_err)?;
        Ok(Response::new(pb::locale::Empty {}))
    }

    async fn timezone_set(
        &self,
        request: Request<pb::locale::TimezoneMessage>,
    ) -> Result<Response<pb::locale::Empty>, Status> {
        let req = request.into_inner();
        self.set_timezone(&req.timezone).map_err(map_err)?;
        Ok(Response::new(pb::locale::Empty {}))
    }
}

fn build_locale_args(assignments: &[pb::locale::LocaleAssignment]) -> Result<Vec<String>> {
    assignments
        .iter()
        .map(|assignment| {
            let key = pb::locale::LocaleMacroKey::try_from(assignment.key)?;
            Ok(format!("{}={}", key.as_str_name(), assignment.value))
        })
        .collect()
}

fn validate_timezone(timezone: &str) -> bool {
    let re = Regex::new(r"^[A-Z][-+a-zA-Z0-9]*(?:/[A-Z][-+a-zA-Z0-9_]*)*$").expect("valid regex");
    re.is_match(timezone)
}

fn run_command<I, S>(name: &str, args: I) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut cmd = Command::new(name);
    for arg in args {
        cmd.arg(arg.as_ref());
    }
    let status = cmd
        .status()
        .with_context(|| format!("failed to run {name}"))?;
    if status.success() {
        Ok(())
    } else {
        bail!("{name} exited with status {status}")
    }
}

fn is_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

fn map_err(err: anyhow::Error) -> Status {
    Status::internal(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_timezone() {
        assert!(validate_timezone("Europe/Helsinki"));
        assert!(validate_timezone("Etc/GMT+3"));
        assert!(!validate_timezone("europe/helsinki"));
        assert!(!validate_timezone("../../etc/passwd"));
    }

    #[test]
    fn builds_locale_args() {
        let args = build_locale_args(&[pb::locale::LocaleAssignment {
            key: pb::locale::LocaleMacroKey::LcTime as i32,
            value: "en_US.UTF-8".to_owned(),
        }])
        .unwrap();

        assert_eq!(args, vec!["LC_TIME=en_US.UTF-8"]);
    }

    #[test]
    fn rejects_empty_locale() {
        let server = LocaleServer::new();
        let err = server.set_locale(&[]).unwrap_err();
        assert!(err.to_string().contains("no locale assignments"));
    }
}
