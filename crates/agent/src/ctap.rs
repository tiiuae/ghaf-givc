// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::Stdio;

use anyhow::{Result, bail};
use givc_common::{pb, tonic::RewrapErr};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tonic::{Request, Response, Status};

pub use pb::ctap::ctap_server::CtapServer as CtapServiceServer;

#[derive(Debug, Default, Clone)]
pub struct CtapService;

impl CtapService {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    fn resolve_program(req: &str) -> Result<&'static str> {
        match req {
            "ctap.ClientPin" => Ok("qctap-client-pin"),
            "ctap.GetInfo" => Ok("qctap-get-info"),
            "u2f.Authenticate" => Ok("qctap-get-assertion"),
            "u2f.Register" => Ok("qctap-make-credential"),
            _ => bail!("Invalid request"),
        }
    }
}

async fn run_helper(program: &Path, args: &[String], payload: &[u8]) -> Result<Vec<u8>, Status> {
    let mut cmd = Command::new(program);
    cmd.args(args).stdin(Stdio::piped()).stdout(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|err| Status::internal(err.to_string()))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| Status::internal("CTAP helper stdin was not piped"))?;
    let payload = payload.to_vec();
    let write_payload = async move {
        let result = stdin.write_all(&payload).await;
        drop(stdin);
        result
    };
    let wait_output = child.wait_with_output();
    let (_write_result, output) = tokio::join!(write_payload, wait_output);
    let output = output.map_err(|err| Status::internal(err.to_string()))?;

    Ok(output.stdout)
}

#[tonic::async_trait]
impl pb::ctap::ctap_server::Ctap for CtapService {
    async fn ctap(
        &self,
        request: Request<pb::ctap::CtapRequest>,
    ) -> Result<Response<pb::ctap::CtapResponse>, Status> {
        let req = request.into_inner();
        let prog = Self::resolve_program(&req.req).rewrap_err()?;

        let output = run_helper(Path::new(prog), &req.args, &req.payload).await?;
        Ok(Response::new(pb::ctap::CtapResponse { output }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestHelper(std::path::PathBuf);

    impl TestHelper {
        fn new(script: &str) -> Self {
            use std::os::unix::fs::PermissionsExt;

            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos();
            let path = std::env::temp_dir()
                .join(format!("givc-ctap-helper-{}-{unique}", std::process::id()));
            std::fs::write(&path, script).expect("write helper script");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
                .expect("make helper executable");
            Self(path)
        }

        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TestHelper {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn maps_known_requests() {
        assert_eq!(
            CtapService::resolve_program("ctap.ClientPin").unwrap(),
            "qctap-client-pin"
        );
        assert_eq!(
            CtapService::resolve_program("u2f.Register").unwrap(),
            "qctap-make-credential"
        );
    }

    #[test]
    fn rejects_unknown_requests() {
        assert!(CtapService::resolve_program("nope").is_err());
    }

    #[tokio::test]
    async fn helper_roundtrips_empty_payload() {
        let helper = TestHelper::new("#!/bin/sh\ncat\n");

        let output = run_helper(helper.path(), &[], &[])
            .await
            .expect("helper output");

        assert!(output.is_empty());
    }

    #[tokio::test]
    async fn helper_roundtrips_payload_larger_than_pipe_capacity() {
        let helper = TestHelper::new("#!/bin/sh\ncat\n");
        let payload = vec![b'x'; 1024 * 1024];

        let output = run_helper(helper.path(), &[], &payload)
            .await
            .expect("helper output");

        assert_eq!(output, payload);
    }

    #[tokio::test]
    async fn helper_stdout_survives_nonzero_exit() {
        let helper = TestHelper::new("#!/bin/sh\nprintf 'helper output'\nexit 17\n");

        let output = run_helper(helper.path(), &[], &[])
            .await
            .expect("helper stdout should be returned");

        assert_eq!(output, b"helper output");
    }
}
