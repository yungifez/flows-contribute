//! Blocking HTTP calls to the work queue (crowd API version 1) and to GitHub.

use serde_json::Value;
use std::time::Duration;

pub fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(30))
        .timeout_read(Duration::from_secs(300))
        .user_agent(&format!("flows-contribute/{}", env!("CARGO_PKG_VERSION")))
        .build()
}

/// A server reply: the HTTP status and the JSON body, if there is one.
#[derive(Debug)]
pub struct Reply {
    pub status: u16,
    pub body: Value,
}

impl Reply {
    pub fn error_code(&self) -> &str {
        self.body.get("error").and_then(Value::as_str).unwrap_or("")
    }

    pub fn message(&self) -> String {
        match self.body.get("message").and_then(Value::as_str) {
            Some(text) => text.to_string(),
            None => format!("The server answered with status {}.", self.status),
        }
    }
}

pub struct Server {
    base: String,
    token: Option<String>,
    agent: ureq::Agent,
}

impl Server {
    pub fn new(base: String, token: Option<String>) -> Self {
        Server {
            base,
            token,
            agent: agent(),
        }
    }

    pub fn get(&self, path: &str) -> Result<Reply, String> {
        self.call("GET", path, None)
    }

    pub fn post(&self, path: &str, body: &Value) -> Result<Reply, String> {
        self.call("POST", path, Some(body))
    }

    fn call(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Reply, String> {
        let mut request = self.agent.request(method, &format!("{}{path}", self.base));
        if let Some(token) = &self.token {
            request = request.set("Authorization", &format!("Bearer {token}"));
        }
        let result = match body {
            Some(body) => request.send_json(body.clone()),
            None => request.call(),
        };
        let response = match result {
            Ok(response) => response,
            Err(ureq::Error::Status(_, response)) => response,
            Err(error) => {
                return Err(format!(
                    "I cannot reach the Flows server ({error}). Check the internet connection and try again."
                ))
            }
        };
        let status = response.status();
        let text = response
            .into_string()
            .map_err(|e| format!("The server reply was cut off ({e}). Try again."))?;
        let body = if text.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).unwrap_or(Value::Null)
        };
        Ok(Reply { status, body })
    }
}
