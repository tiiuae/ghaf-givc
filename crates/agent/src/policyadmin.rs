// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Result, bail};
use givc_common::{pb, tonic::RewrapErr};
use sha2::{Digest, Sha256};
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex as AsyncMutex;
use tokio_stream::StreamExt;
use tonic::{Request, Response, Status};

pub use pb::policyadmin::policy_admin_server::PolicyAdminServer as PolicyAdminServerServer;

static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PolicyDetail {
    #[serde(rename = "Destination")]
    destination: String,
    #[serde(rename = "Sha")]
    sha: String,
}

#[derive(Debug)]
pub struct PolicyAdminServer {
    store_path: PathBuf,
    policy_map: Mutex<HashMap<String, PolicyDetail>>,
    update_lock: AsyncMutex<()>,
}

impl PolicyAdminServer {
    #[must_use]
    pub fn new(store_path: String, policies: HashMap<String, String>) -> Self {
        let store_path = PathBuf::from(store_path);

        let configured_policy_map: HashMap<_, _> = policies
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
        let policy_map = std::fs::read_to_string(store_path.join("policies.json"))
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or(configured_policy_map);

        Self {
            store_path,
            policy_map: Mutex::new(policy_map),
            update_lock: AsyncMutex::new(()),
        }
    }

    fn temp_dir(&self) -> PathBuf {
        self.store_path.join(".temp")
    }

    fn policies_file(&self) -> PathBuf {
        self.store_path.join("policies.json")
    }

    async fn update_policy(&self, policy_name: &str, pulled_policy_path: &Path) -> Result<()> {
        let _update_guard = self.update_lock.lock().await;
        let (destination, current_sha) = {
            let policy_map = self.policy_map.lock().expect("policy map poisoned");
            let Some(policy) = policy_map.get(policy_name) else {
                bail!("Unknown policy {policy_name}.");
            };
            (policy.destination.clone(), policy.sha.clone())
        };

        let pulled_hash = file_hash(pulled_policy_path).await?;
        if current_sha == pulled_hash {
            return Ok(());
        }

        let policy_dir = self.store_path.join(policy_name);
        let policy_file = policy_dir.join("policy.bin");
        fs::create_dir_all(&policy_dir).await?;

        if destination.is_empty() {
            fs::rename(pulled_policy_path, &policy_file).await?;
        } else {
            copy_policy(pulled_policy_path, Path::new(&destination)).await?;
            fs::rename(pulled_policy_path, &policy_file).await?;
        }

        let json = {
            let mut policy_map = self.policy_map.lock().expect("policy map poisoned");
            let Some(policy) = policy_map.get_mut(policy_name) else {
                bail!("Unknown policy {policy_name}.");
            };
            policy.sha = pulled_hash;
            serde_json::to_string_pretty(&*policy_map)?
        };
        let policies_file = self.policies_file();
        let temp_file = self.store_path.join(format!(
            "policies.json-{}-{}.tmp",
            std::process::id(),
            TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_file)
            .await?;
        file.write_all(json.as_bytes()).await?;
        file.flush().await?;
        drop(file);
        fs::rename(temp_file, policies_file).await?;
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

                let tmp = create_temp_policy_file(&self.temp_dir())
                    .await
                    .rewrap_err()?;
                temp_file = Some(tmp);
            }

            if let Some(path) = temp_file.as_ref()
                && !req.policy_chunk.is_empty()
            {
                let mut file = fs::OpenOptions::new()
                    .append(true)
                    .open(path)
                    .await
                    .map_err(|err| Status::internal(err.to_string()))?;
                file.write_all(&req.policy_chunk)
                    .await
                    .map_err(|err| Status::internal(err.to_string()))?;
            }
        }

        let Some(policy_file_path) = temp_file else {
            return Err(Status::invalid_argument(
                "policy-admin: policy stream is empty",
            ));
        };

        let update_result = self.update_policy(&policy_name, &policy_file_path).await;
        let _ = fs::remove_file(&policy_file_path).await;
        update_result.rewrap_err()?;
        Ok(Response::new(pb::policyadmin::Status {
            status: "Success".to_owned(),
        }))
    }
}

async fn create_temp_policy_file(temp_dir: &Path) -> Result<PathBuf> {
    fs::create_dir_all(temp_dir).await?;
    loop {
        let path = temp_dir.join(format!(
            "policy.bin-{}-{}",
            std::process::id(),
            TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .await
        {
            Ok(_) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
}

async fn file_hash(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buf = [0_u8; 4096];
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

async fn copy_policy(src: &Path, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).await?;
    }
    let mut dst = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(dest)
        .await?;
    let mut src = fs::File::open(src).await?;
    tokio::io::copy(&mut src, &mut dst).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::sync::mpsc;
    use tokio_stream::wrappers::{ReceiverStream, TcpListenerStream};

    fn test_store() -> PathBuf {
        std::env::temp_dir().join(format!(
            "givc-policy-admin-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    async fn stream_policy(
        address: std::net::SocketAddr,
        policy_name: &str,
        payload: Vec<u8>,
    ) -> Result<pb::policyadmin::Status, tonic::Status> {
        let mut client = pb::policyadmin::policy_admin_client::PolicyAdminClient::connect(format!(
            "http://{address}"
        ))
        .await
        .unwrap();
        let (tx, rx) = mpsc::channel(4);
        tx.send(pb::policyadmin::StreamPolicyRequest {
            policy_name: policy_name.to_owned(),
            policy_chunk: payload,
        })
        .await
        .unwrap();
        drop(tx);
        client
            .stream_policy(ReceiverStream::new(rx))
            .await
            .map(tonic::Response::into_inner)
    }

    #[tokio::test]
    async fn hashes_file() {
        let path = std::env::temp_dir().join(format!("givc-policy-{}", std::process::id()));
        fs::write(&path, b"hello").await.unwrap();
        assert_eq!(
            file_hash(&path).await.unwrap(),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        let _ = fs::remove_file(&path).await;
    }

    #[tokio::test]
    async fn loads_persisted_policy_state_after_reconstruction() {
        let store = test_store();
        let server = PolicyAdminServer::new(
            store.to_string_lossy().into_owned(),
            HashMap::from([("network".to_owned(), String::new())]),
        );
        let pulled = store.join("incoming.bin");
        fs::create_dir_all(&store).await.unwrap();
        fs::write(&pulled, b"policy-v1").await.unwrap();
        server.update_policy("network", &pulled).await.unwrap();

        let restarted =
            PolicyAdminServer::new(store.to_string_lossy().into_owned(), HashMap::new());
        let (destination, sha) = {
            let persisted = restarted.policy_map.lock().unwrap();
            (
                persisted["network"].destination.clone(),
                persisted["network"].sha.clone(),
            )
        };
        assert_eq!(destination, "");
        assert_eq!(
            sha,
            file_hash(&store.join("network/policy.bin")).await.unwrap()
        );
        fs::remove_dir_all(store).await.unwrap();
    }

    #[tokio::test]
    async fn persisted_policy_fields_keep_go_json_casing() {
        let store = test_store();
        let server = PolicyAdminServer::new(
            store.to_string_lossy().into_owned(),
            HashMap::from([("network".to_owned(), String::new())]),
        );
        let pulled = store.join("incoming.bin");
        fs::create_dir_all(&store).await.unwrap();
        fs::write(&pulled, b"policy-v1").await.unwrap();
        server.update_policy("network", &pulled).await.unwrap();

        let json: serde_json::Value =
            serde_json::from_slice(&fs::read(store.join("policies.json")).await.unwrap()).unwrap();
        let policy = &json["network"];
        assert!(policy.get("Destination").is_some());
        assert!(policy.get("Sha").is_some());
        assert!(policy.get("destination").is_none());
        assert!(policy.get("sha").is_none());

        fs::remove_dir_all(store).await.unwrap();
    }

    #[tokio::test]
    async fn simultaneous_policy_streams_keep_separate_artifacts() {
        let store = test_store();
        let service = PolicyAdminServer::new(
            store.to_string_lossy().into_owned(),
            HashMap::from([
                ("first".to_owned(), String::new()),
                ("second".to_owned(), String::new()),
            ]),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(PolicyAdminServerServer::new(service))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .unwrap();
        });
        let first = vec![b'a'; 256 * 1024];
        let second = vec![b'b'; 256 * 1024];
        let (first_result, second_result) = tokio::join!(
            stream_policy(address, "first", first.clone()),
            stream_policy(address, "second", second.clone()),
        );
        server.abort();

        assert_eq!(first_result.unwrap().status, "Success");
        assert_eq!(second_result.unwrap().status, "Success");
        assert_eq!(
            fs::read(store.join("first/policy.bin")).await.unwrap(),
            first
        );
        assert_eq!(
            fs::read(store.join("second/policy.bin")).await.unwrap(),
            second
        );
        let persisted: serde_json::Value =
            serde_json::from_slice(&fs::read(store.join("policies.json")).await.unwrap()).unwrap();
        assert!(persisted.get("first").is_some());
        assert!(persisted.get("second").is_some());
        fs::remove_dir_all(store).await.unwrap();
    }
}
