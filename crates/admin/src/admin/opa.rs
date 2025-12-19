use anyhow::anyhow;
use reqwest::Client;
use tracing::{debug, error, info};

#[derive(Debug)]

pub struct OPAPolicyClient {
    url: String,
}

impl OPAPolicyClient {
    pub fn new(serverurl: String) -> Self {
        debug!(
            "opa-policy-client: creating interface to policy server with url: {}",
            serverurl
        );
        Self { url: serverurl }
    }

    /* Sends request to OPA server and returns the response as a string */
    pub async fn request(&self, query: &str, policy_path: &str) -> anyhow::Result<String> {
        let opa_url = format!("{}{}", self.url, policy_path);
        debug!(
            "opa-policy-client: OPA query: {:?}, url: {:?} ",
            query, opa_url
        );

        let client = Client::new();

        let res = match client.post(&opa_url).body(query.to_string()).send().await {
            Ok(response) => response,
            Err(e) => {
                error!(
                    "opa-policy-client: failed to send request to OPA server: {}",
                    e
                );
                return Ok("{}".to_string());
            }
        };

        let body_string = match res.text().await {
            Ok(text) => text,
            Err(e) => {
                error!("opa-policy-client: failed to read body string: {}", e);
                return Ok("{}".to_string());
            }
        };

        Ok(body_string)
    }

    /* Converts CLI queries to opa request and sends to OPA server */
    pub async fn evaluate_query(&self, query: &str, policy_path: &str) -> anyhow::Result<String> {
        if let Some(json_payload) = query.strip_prefix("json:") {
            debug!("opa-policy-client: detected 'json:' prefix.");
            let result = self.request(json_payload, policy_path).await?;
            Ok(result)
        } else if let Some(command_line) = query.strip_prefix("cmd:") {
            debug!("opa-policy-client: detected 'cmd:' prefix.");
            let result = self.handle_cmds(command_line).await?;
            Ok(result)
        } else {
            Err(anyhow!(
                "opa-policy-client: unrecognized query prefix, expected 'json:' or 'cmd:'"
            ))
        }
    }

    /* Splits custom OPA commands into command and arguments */
    pub async fn split_cmd_and_args<'a>(&self, cmdstr: &'a str) -> Option<(&'a str, &'a str)> {
        let cmdstr = cmdstr.trim();
        cmdstr.split_once(' ').or(Some((cmdstr, "")))
    }

    /* Handles custom OPA commands */
    pub async fn handle_cmds(&self, cmdstr: &str) -> anyhow::Result<String> {
        if let Some((cmd, args)) = self.split_cmd_and_args(cmdstr).await {
            info!("opa-policy-client: command:{}, args: {}", cmd, args);
            match cmd {
                "fetch" => self.run_fetch_rules(args).await,
                /* Add other commands here */
                _ => Err(anyhow!("opa-policy-client: unknown command: {}", cmd)),
            }
        } else {
            Err(anyhow!("opa-policy-client: invalid command {}", cmdstr))
        }
    }

    /* Command Handlers */
    async fn run_fetch_rules(&self, args: &str) -> anyhow::Result<String> {
        let rule_path = args.trim();
        let json_string = format!(r#"{{"input":{{}}}}"#,);

        let result = self.request(json_string.as_str(), rule_path).await?;
        Ok(result)
    }
}
