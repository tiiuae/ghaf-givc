// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::Write;
use std::os::unix::fs::FileTypeExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use givc_common::pb;
use tonic::{Request, Response, Status};

pub use pb::notify::user_notification_service_server::UserNotificationServiceServer;

#[derive(Debug, Clone)]
pub struct UserNotifierServer {
    socket_dir: PathBuf,
}

impl UserNotifierServer {
    #[must_use]
    pub fn new(socket_dir: String) -> Self {
        Self {
            socket_dir: PathBuf::from(socket_dir),
        }
    }

    fn broadcast_notification(&self, json_message: &[u8]) -> Result<()> {
        let entries = match fs::read_dir(&self.socket_dir) {
            Ok(entries) => entries,
            Err(err) => {
                return Err(err).with_context(|| {
                    format!("could not read directory '{}'", self.socket_dir.display())
                });
            }
        };

        let mut sockets = Vec::new();
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            let path = entry.path();
            let Ok(meta) = fs::metadata(&path) else {
                continue;
            };
            if meta.file_type().is_socket() {
                sockets.push(path);
            }
        }

        if sockets.is_empty() {
            return Ok(());
        }

        for socket in sockets {
            let _ = send_message_to_socket(&socket, json_message);
        }
        Ok(())
    }
}

#[tonic::async_trait]
impl pb::notify::user_notification_service_server::UserNotificationService for UserNotifierServer {
    async fn notify_user(
        &self,
        request: Request<pb::notify::UserNotification>,
    ) -> Result<Response<pb::notify::Status>, Status> {
        let notification = request.into_inner();
        let json = notification_to_json(&notification).into_bytes();
        self.broadcast_notification(&json).map_err(map_err)?;
        Ok(Response::new(pb::notify::Status {
            status: "Notification sent".to_owned(),
        }))
    }
}

fn notification_to_json(notification: &pb::notify::UserNotification) -> String {
    let urgency = pb::notify::UrgencyLevel::try_from(notification.urgency)
        .map(|level| level.as_str_name())
        .unwrap_or("NORMAL");
    serde_json::json!({
        "event": notification.event,
        "title": notification.title,
        "urgency": urgency,
        "icon": notification.icon,
        "message": notification.message,
    })
    .to_string()
}

fn send_message_to_socket(socket_path: &Path, data: &[u8]) -> Result<()> {
    let mut conn = UnixStream::connect(socket_path)?;
    conn.write_all(data)?;
    Ok(())
}

fn map_err(err: anyhow::Error) -> Status {
    Status::internal(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::sync::mpsc;
    use std::thread;

    #[test]
    fn formats_json() {
        let json = notification_to_json(&pb::notify::UserNotification {
            event: "event".to_owned(),
            title: "title".to_owned(),
            urgency: pb::notify::UrgencyLevel::Critical as i32,
            icon: "icon".to_owned(),
            message: "msg".to_owned(),
        });
        assert!(json.contains(r#""event":"event""#));
        assert!(json.contains(r#""urgency":"CRITICAL""#));
    }

    #[test]
    fn broadcasts_to_socket() {
        let base = std::env::temp_dir().join(format!("givc-notify-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let socket_path = base.join("user.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();
        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = Vec::new();
            use std::io::Read;
            stream.read_to_end(&mut buf).unwrap();
            tx.send(buf).unwrap();
        });

        let server = UserNotifierServer::new(base.to_string_lossy().into_owned());
        server.broadcast_notification(br#"{"event":"x"}"#).unwrap();

        let data = rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        assert_eq!(data, br#"{"event":"x"}"#);

        let _ = fs::remove_dir_all(&base);
    }
}
