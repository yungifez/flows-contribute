//! Local checks on the agent's labels before upload. The server checks them again.

use regex::Regex;
use serde_json::{Map, Value};

pub const FIELDS: [&str; 7] = [
    "unit_id",
    "disposition",
    "flow_id",
    "note",
    "handler",
    "files_read",
    "behavior_summary",
];
pub const DISPOSITIONS: [&str; 4] = ["MATCH", "VARIANT", "UNKNOWN", "NOT_FLOW"];
pub const REMOVED: &str = "[removed by client: possible secret]";
const TEXT_LIMIT: usize = 2000;
const HANDLER_LIMIT: usize = 300;
const PATH_LIMIT: usize = 300;
const FILES_LIMIT: usize = 50;

/// Check the agent's answer: an object with `labels` that covers the assigned units in order.
pub fn validate(answer: &Value, expected: &[String]) -> Result<Vec<Map<String, Value>>, String> {
    let labels = answer
        .get("labels")
        .and_then(Value::as_array)
        .ok_or("the answer has no labels list")?;
    let ids: Vec<&str> = labels
        .iter()
        .map(|label| label.get("unit_id").and_then(Value::as_str).unwrap_or(""))
        .collect();
    if ids != expected.iter().map(String::as_str).collect::<Vec<_>>() {
        return Err("the labels do not cover the assigned units in order".to_string());
    }
    let mut out = Vec::new();
    for label in labels {
        let object = label.as_object().ok_or("a label is not an object")?;
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        let mut wanted = FIELDS.to_vec();
        wanted.sort_unstable();
        if keys != wanted {
            return Err(format!("a label must have exactly the fields {FIELDS:?}"));
        }
        let disposition = object["disposition"].as_str().unwrap_or("");
        if !DISPOSITIONS.contains(&disposition) {
            return Err(format!("a label has an unknown disposition: {disposition}"));
        }
        if !object["files_read"].is_array() {
            return Err("files_read must be a list".to_string());
        }
        out.push(object.clone());
    }
    Ok(out)
}

fn secret_patterns() -> Vec<Regex> {
    [
        r"-----BEGIN [A-Z ]*PRIVATE KEY-----",
        r"AKIA[0-9A-Z]{16}",
        r"\bgh[pousr]_[A-Za-z0-9]{20,}",
        r"\bgithub_pat_[A-Za-z0-9_]{20,}",
        r"\bsk-[A-Za-z0-9_-]{20,}",
        r"\bxox[abprs]-[A-Za-z0-9-]{10,}",
    ]
    .iter()
    .map(|pattern| Regex::new(pattern).unwrap())
    .collect()
}

pub fn has_secret(text: &str) -> bool {
    secret_patterns()
        .iter()
        .any(|pattern| pattern.is_match(text))
}

fn truncate(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= PATH_LIMIT
        && !path.starts_with('/')
        && !path.starts_with('\\')
        && !path.contains(':')
        && !path.split(['/', '\\']).any(|part| part == "..")
}

/// Remove text that looks like a secret, and fit fields to the server limits. Returns how many
/// fields or paths were removed because they looked like a secret.
pub fn scrub(labels: &mut [Map<String, Value>]) -> usize {
    let mut removed = 0;
    for label in labels.iter_mut() {
        for (field, limit) in [
            ("note", TEXT_LIMIT),
            ("behavior_summary", TEXT_LIMIT),
            ("handler", HANDLER_LIMIT),
        ] {
            if let Some(text) = label.get(field).and_then(Value::as_str) {
                let clean = if has_secret(text) {
                    removed += 1;
                    REMOVED.to_string()
                } else {
                    truncate(text, limit)
                };
                label.insert(field.to_string(), Value::String(clean));
            }
        }
        let files: Vec<Value> = label["files_read"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|value| value.as_str().map(str::to_string))
            .filter(|path| {
                if has_secret(path) {
                    removed += 1;
                    return false;
                }
                safe_path(path)
            })
            .take(FILES_LIMIT)
            .map(Value::String)
            .collect();
        label.insert("files_read".to_string(), Value::Array(files));
    }
    removed
}

/// If the agent output says a usage limit stopped it, return a short text for the contributor.
pub fn usage_limit(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    if !["usage limit", "try again at", "rate limit"]
        .iter()
        .any(|key| lower.contains(key))
    {
        return None;
    }
    if let Some(found) = Regex::new("(?i)try again at").unwrap().find(text) {
        let rest: String = text[found.start()..]
            .chars()
            .take_while(|c| !matches!(c, '"' | '\n' | '\r' | '\\'))
            .take(80)
            .collect();
        return Some(rest.trim_end_matches(['.', ' ']).to_string() + ".");
    }
    Some(String::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn label(id: &str) -> Value {
        json!({"unit_id": id, "disposition": "MATCH", "flow_id": "resource.update", "note": "",
               "handler": "A@b", "files_read": ["app/A.php"], "behavior_summary": "Updates A."})
    }

    #[test]
    fn validates_order_and_fields() {
        let ids = vec!["u1".to_string(), "u2".to_string()];
        assert!(validate(&json!({"labels": [label("u1"), label("u2")]}), &ids).is_ok());
        assert!(validate(&json!({"labels": [label("u2"), label("u1")]}), &ids).is_err());
        assert!(validate(&json!({"labels": [label("u1")]}), &ids).is_err());
        let mut extra = label("u1");
        extra["x"] = json!(1);
        assert!(validate(&json!({"labels": [extra, label("u2")]}), &ids).is_err());
        let mut bad = label("u1");
        bad["disposition"] = json!("MAYBE");
        assert!(validate(&json!({"labels": [bad, label("u2")]}), &ids).is_err());
        assert!(validate(&json!({}), &ids).is_err());
    }

    #[test]
    fn scrubs_secrets_and_bad_paths() {
        let mut value = label("u1");
        value["note"] = json!("key AKIAABCDEFGHIJKLMNOP found");
        value["behavior_summary"] = json!("x".repeat(3000));
        value["files_read"] = json!([
            "app/A.php",
            "../../etc/passwd",
            "/etc/passwd",
            "ghp_abcdefghijklmnopqrstuvwxyz0123456789"
        ]);
        let mut labels = vec![value.as_object().unwrap().clone()];
        assert_eq!(scrub(&mut labels), 2);
        assert_eq!(labels[0]["note"], json!(REMOVED));
        assert_eq!(labels[0]["behavior_summary"].as_str().unwrap().len(), 2000);
        assert_eq!(labels[0]["files_read"], json!(["app/A.php"]));
        assert_eq!(labels[0]["handler"], json!("A@b"));
    }

    #[test]
    fn finds_secret_patterns() {
        assert!(has_secret("-----BEGIN OPENSSH PRIVATE KEY-----"));
        assert!(has_secret("token sk-abcdefghijklmnopqrstuvwx"));
        assert!(has_secret("xoxb-1234567890-abc"));
        assert!(!has_secret("ApiKeyController@reset uses a stored secret"));
    }

    #[test]
    fn detects_usage_limits() {
        assert_eq!(
            usage_limit("ERROR: You've hit your usage limit. Try again at 4:05 PM.\n"),
            Some("Try again at 4:05 PM.".to_string())
        );
        assert_eq!(usage_limit("429 rate limit exceeded"), Some(String::new()));
        assert_eq!(usage_limit("all good"), None);
    }
}
