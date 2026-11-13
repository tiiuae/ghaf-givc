// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use givc_common::{pb, tonic::RewrapErr};
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tonic::{Request, Response, Status};
use tracing::error;

pub use pb::notify::user_notification_service_server::UserNotificationServiceServer;

#[derive(Debug, Clone)]
pub struct UserNotifierServer {
    socket_dir: PathBuf,
}

impl UserNotifierServer {
    #[must_use]
    pub fn new(socket_dir: PathBuf) -> Self {
        Self { socket_dir }
    }

    async fn broadcast_notification(&self, json_message: &[u8]) -> Result<()> {
        let mut entries = tokio::fs::read_dir(&self.socket_dir)
            .await
            .with_context(|| format!("could not read directory '{}'", self.socket_dir.display()))?;

        while let Some(entry) = entries.next_entry().await? {
            if !entry.file_type().await?.is_socket() {
                continue;
            }

            if let Err(e) = send_message_to_socket(&entry.path(), json_message).await {
                error!("error sending message to socket {e}")
            };
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
        self.broadcast_notification(&json).await.rewrap_err()?;
        Ok(Response::new(pb::notify::Status {
            status: "Notification sent".to_owned(),
        }))
    }
}

fn notification_to_json(notification: &pb::notify::UserNotification) -> String {
    let mut json = serde_json::Map::new();
    for (key, value) in [
        ("Event", &notification.event),
        ("Title", &notification.title),
        ("Icon", &notification.icon),
        ("Message", &notification.message),
    ] {
        if !value.is_empty() {
            json.insert(key.to_owned(), serde_json::Value::String(value.clone()));
        }
    }
    if notification.urgency != pb::notify::UrgencyLevel::Low as i32 {
        let urgency = pb::notify::UrgencyLevel::try_from(notification.urgency).map_or_else(
            |_| serde_json::json!(notification.urgency),
            |level| serde_json::json!(level.as_str_name()),
        );
        json.insert("Urgency".to_owned(), urgency);
    }
    serde_json::Value::Object(json).to_string()
}

async fn send_message_to_socket(socket_path: &Path, data: &[u8]) -> Result<()> {
    let mut conn = UnixStream::connect(socket_path).await?;
    conn.write_all(data).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tokio::io::AsyncReadExt;
    use tokio::net::UnixListener;

    #[test]
    fn formats_json() {
        let json = notification_to_json(&pb::notify::UserNotification {
            event: "event".to_owned(),
            title: "title".to_owned(),
            urgency: pb::notify::UrgencyLevel::Critical as i32,
            icon: "icon".to_owned(),
            message: "msg".to_owned(),
        });
        assert!(json.contains(r#""Event":"event""#));
        assert!(json.contains(r#""Urgency":"CRITICAL""#));
    }

    #[test]
    fn formats_notification_as_protojson() {
        let json = notification_to_json(&pb::notify::UserNotification {
            event: "event".to_owned(),
            title: "title".to_owned(),
            urgency: pb::notify::UrgencyLevel::Critical as i32,
            icon: "icon".to_owned(),
            message: "msg".to_owned(),
        });
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "Event": "event", "Title": "title", "Urgency": "CRITICAL",
                "Icon": "icon", "Message": "msg"
            })
        );
    }

    #[test]
    fn protojson_omits_default_fields() {
        let json = notification_to_json(&pb::notify::UserNotification::default());
        assert_eq!(json, "{}");
    }

    #[tokio::test]
    async fn broadcasts_to_socket() {
        let base = std::env::temp_dir().join(format!("givc-notify-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let socket_path = base.join("user.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();
        let receiver = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = Vec::new();
            stream.read_to_end(&mut buf).await.unwrap();
            buf
        });

        let server = UserNotifierServer::new(base.clone());
        server
            .broadcast_notification(br#"{"event":"x"}"#)
            .await
            .unwrap();

        let data = receiver.await.unwrap();
        assert_eq!(data, br#"{"event":"x"}"#);

        let _ = fs::remove_dir_all(&base);
    }
}
