//use anyhow::{anyhow, bail, Context, Result};
//use serde_json::json;
use anyhow::anyhow;
use serde_json::Value;
use tracing::info;
use tracing::{debug, error};

#[derive(Debug)]

pub struct PolicyServer {
    url: String,
}

impl PolicyServer {
    pub fn new(serverurl: String) -> Self {
        debug!(
            "Creating Interface to Policy Server with URL: {}",
            serverurl
        );
        Self { url: serverurl }
    }

    pub async fn request(&self, query: &str, policy_path: &str) -> anyhow::Result<String> {
        let opa_url = format!("{}{}", self.url, policy_path);
        info!("Policy QUERY: {:#?}, URL: {:#?} ", query, opa_url);

        let body = surf::Body::from_string((&query).to_string());

        let request = surf::post(opa_url).body(body);
        let mut res = match request.await {
            Ok(response) => response,
            Err(e) => {
                error!("Failed to send request to OPA server: {}", e);
                return Ok("{}".to_string());
            }
        };

        let body_string = match res.body_string().await {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to read body string: {}", e);
                return Ok("{}".to_string());
            }
        };
        Ok(body_string)
    }

    pub async fn evaluate_query(&self, query: &str, policy_path: &str) -> anyhow::Result<String> {
        if let Some(json_payload) = query.strip_prefix("json:") {
            debug!("Detected 'json:' prefix.");
            let result = self.request(json_payload, policy_path).await?;
            Ok(result)
        } else if let Some(command_line) = query.strip_prefix("cmd:") {
            debug!("Detected 'cmd:' prefix.");
            let result = self.handle_cmds(command_line).await?;
            Ok(result)
        } else {
            Err(anyhow!(
                "Unrecognized query prefix. Expected 'json:' or 'cmd:'"
            ))
        }
    }

    pub async fn split_cmd_and_args<'a>(&self, cmdstr: &'a str) -> Option<(&'a str, &'a str)> {
        let mut parts = cmdstr.trim().splitn(2, ' ');
        let cmd = parts.next()?;
        let args = parts.next().unwrap_or("");
        Some((cmd, args))
    }

    pub async fn handle_cmds(&self, cmdstr: &str) -> anyhow::Result<String> {
        if let Some((cmd, args)) = self.split_cmd_and_args(cmdstr).await {
            info!("First word: {}", cmd);
            info!("Remaining: {}", args);
            match cmd {
                "fetch" => self.run_fetch_rules(args).await,
                // Add other commands here
                _ => Err(anyhow!("Unknown command: {}", cmd)),
            }
        } else {
            error!("Invalid command! {}", cmdstr);
            Err(anyhow!("Invalid command {}", cmdstr))
        }
    }

    // --- Command Handlers ---

    async fn run_fetch_rules(&self, args: &str) -> anyhow::Result<String> {
        let rule_path = args.trim();
        let json_string = format!(r#"{{"input":{{}}}}"#,);

        let result = self.request(json_string.as_str(), rule_path).await?;
        Ok(result)
    }
}
