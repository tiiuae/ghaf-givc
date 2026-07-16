// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Result, bail};
use givc_common::pb;
use sha2::{Digest, Sha256};
use tokio_stream::StreamExt;
use tonic::{Request, Response, Status};

pub use pb::policyadmin::policy_admin_server::PolicyAdminServer as PolicyAdminServerServer;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PolicyDetail {
    destination: String,
    sha: String,
}

#[derive(Debug)]
pub struct PolicyAdminServer {
    store_path: PathBuf,
    policy_map: Mutex<HashMap<String, PolicyDetail>>,
}

impl PolicyAdminServer {
    #[must_use]
    pub fn new(store_path: String, policies: HashMap<String, String>) -> Self {
        let store_path = PathBuf::from(store_path);
        let temp_dir = store_path.join(".temp");
        let _ = fs::create_dir_all(&temp_dir);

        let policy_map = policies
            .into_iter()
            .map(|(name, destination)| {
                (
                    name,
                    PolicyDetail {
                        destination,
                        sha: String::new(),
                    },
                )
            })
            .collect();

        Self {
            store_path,
            policy_map: Mutex::new(policy_map),
        }
    }

    fn temp_dir(&self) -> PathBuf {
        self.store_path.join(".temp")
    }

    fn policies_file(&self) -> PathBuf {
        self.store_path.join("policies.json")
    }

    fn update_policy(&self, policy_name: &str, pulled_policy_path: &Path) -> Result<()> {
        let mut policy_map = self.policy_map.lock().expect("policy map poisoned");
        let Some(policy) = policy_map.get_mut(policy_name) else {
            bail!("Unknown policy {policy_name}.");
        };

        let policy_dir = self.store_path.join(policy_name);
        let policy_file = policy_dir.join("policy.bin");
        fs::create_dir_all(&policy_dir)?;

        let pulled_hash = file_hash(pulled_policy_path)?;
        if policy.sha == pulled_hash {
            return Ok(());
        }

        if policy.destination.is_empty() {
            fs::rename(pulled_policy_path, &policy_file)?;
        } else {
            copy_policy(pulled_policy_path, Path::new(&policy.destination))?;
            fs::rename(pulled_policy_path, &policy_file)?;
        }

        policy.sha = pulled_hash;
        save_policy_map(&self.policies_file(), &policy_map)?;
        Ok(())
    }
}

#[tonic::async_trait]
impl pb::policyadmin::policy_admin_server::PolicyAdmin for PolicyAdminServer {
    async fn stream_policy(
        &self,
        request: Request<tonic::Streaming<pb::policyadmin::StreamPolicyRequest>>,
    ) -> Result<Response<pb::policyadmin::Status>, Status> {
        let mut stream = request.into_inner();
        let mut policy_name = String::new();
        let mut temp_file: Option<PathBuf> = None;
        let mut first_chunk = true;

        while let Some(chunk) = stream.next().await {
            let req = chunk.map_err(|err| Status::internal(err.to_string()))?;

            if first_chunk {
                first_chunk = false;
                policy_name = req.policy_name;
                if policy_name.is_empty() {
                    return Err(Status::invalid_argument("policy-admin: policy name is nil"));
                }

                let tmp = create_temp_policy_file(&self.temp_dir()).map_err(map_err)?;
                temp_file = Some(tmp);
            }

            if let Some(path) = temp_file.as_ref() {
                if !req.policy_chunk.is_empty() {
                    let mut file = fs::OpenOptions::new().append(true).open(path)?;
                    file.write_all(&req.policy_chunk)?;
                }
            }
        }

        let Some(policy_file_path) = temp_file else {
            return Err(Status::invalid_argument(
                "policy-admin: policy stream is empty",
            ));
        };

        self.update_policy(&policy_name, &policy_file_path)
            .map_err(map_err)?;

        let _ = fs::remove_file(&policy_file_path);
        Ok(Response::new(pb::policyadmin::Status {
            status: "Success".to_owned(),
        }))
    }
}

fn create_temp_policy_file(temp_dir: &Path) -> Result<PathBuf> {
    fs::create_dir_all(temp_dir)?;
    let path = temp_dir.join(format!("policy.bin-{}", std::process::id()));
    let _ = fs::File::create(&path)?;
    Ok(path)
}

fn file_hash(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0_u8; 4096];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn copy_policy(src: &Path, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut dst = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(dest)?;
    let mut src = fs::File::open(src)?;
    std::io::copy(&mut src, &mut dst)?;
    Ok(())
}

fn save_policy_map(path: &Path, map: &HashMap<String, PolicyDetail>) -> Result<()> {
    let json = serde_json::to_string_pretty(map)?;
    fs::write(path, json)?;
    Ok(())
}

fn map_err(err: anyhow::Error) -> Status {
    Status::internal(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_file() {
        let path = std::env::temp_dir().join(format!("givc-policy-{}", std::process::id()));
        fs::write(&path, b"hello").unwrap();
        assert_eq!(
            file_hash(&path).unwrap(),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        let _ = fs::remove_file(&path);
    }
}
