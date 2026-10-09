//! GitHub device flow login. The client asks for no scopes. The GitHub token goes to the Flows
//! server once, to prove the account, and is not stored.

use crate::api;
use serde_json::Value;
use std::time::{Duration, Instant};

pub fn device_login(github: &str, client_id: &str) -> Result<String, String> {
    let agent = api::agent();
    let start: Value = agent
        .post(&format!("{github}/login/device/code"))
        .set("Accept", "application/json")
        .send_form(&[("client_id", client_id)])
        .map_err(|e| format!("I cannot start the GitHub login ({e})."))?
        .into_json()
        .map_err(|e| format!("GitHub gave an unexpected answer ({e})."))?;
    let text = |key: &str| {
        start
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let (device_code, user_code, uri) = (
        text("device_code"),
        text("user_code"),
        text("verification_uri"),
    );
    if device_code.is_empty() || user_code.is_empty() {
        return Err(format!("GitHub did not start the login: {start}"));
    }
    let mut interval = start.get("interval").and_then(Value::as_u64).unwrap_or(5);
    let expires = start
        .get("expires_in")
        .and_then(Value::as_u64)
        .unwrap_or(900);
    println!("1. Open this page in a browser: {uri}");
    println!("2. Enter this code: {user_code}");
    println!("I wait here until you finish.");
    let deadline = Instant::now() + Duration::from_secs(expires);
    loop {
        if Instant::now() > deadline {
            return Err("The login code expired. Run the login command again.".to_string());
        }
        std::thread::sleep(Duration::from_secs(interval));
        let reply: Value = agent
            .post(&format!("{github}/login/oauth/access_token"))
            .set("Accept", "application/json")
            .send_form(&[
                ("client_id", client_id),
                ("device_code", &device_code),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
            .map_err(|e| format!("I cannot reach GitHub ({e})."))?
            .into_json()
            .map_err(|e| format!("GitHub gave an unexpected answer ({e})."))?;
        if let Some(token) = reply.get("access_token").and_then(Value::as_str) {
            return Ok(token.to_string());
        }
        match reply.get("error").and_then(Value::as_str).unwrap_or("") {
            "authorization_pending" => {}
            "slow_down" => {
                interval = reply
                    .get("interval")
                    .and_then(Value::as_u64)
                    .unwrap_or(interval + 5)
            }
            "expired_token" => {
                return Err("The login code expired. Run the login command again.".to_string())
            }
            "access_denied" => return Err("The login was cancelled on GitHub.".to_string()),
            other => return Err(format!("GitHub refused the login ({other}).")),
        }
    }
}
