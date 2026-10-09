//! Run the contributor's coding agent on one batch. Codex runs in its read-only sandbox. Claude
//! Code runs in restricted mode with only the read tools, limited to the batch and snapshot folders.

use crate::labels::usage_limit;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Codex,
    Claude,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Codex => "codex",
            Kind::Claude => "claude",
        }
    }

    pub fn parse(value: &str) -> Option<Kind> {
        match value {
            "codex" => Some(Kind::Codex),
            "claude" => Some(Kind::Claude),
            _ => None,
        }
    }
}

pub struct Agent {
    pub kind: Kind,
    pub program: String,
    pub version: String,
}

/// Find an installed agent. On Windows, npm installs `<name>.cmd`.
pub fn find(kind: Kind) -> Option<Agent> {
    let mut names = vec![kind.name().to_string()];
    if cfg!(windows) {
        names.push(format!("{}.cmd", kind.name()));
    }
    for program in names {
        if let Ok(output) = Command::new(&program)
            .arg("--version")
            .stdin(Stdio::null())
            .output()
        {
            if output.status.success() {
                let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
                return Some(Agent {
                    kind,
                    program,
                    version,
                });
            }
        }
    }
    None
}

pub fn codex_logged_in(agent: &Agent) -> bool {
    Command::new(&agent.program)
        .args(["login", "status"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

pub enum Outcome {
    Answer {
        answer: Value,
        events_sha256: String,
        usage: Value,
    },
    UsageLimit(String),
}

pub struct Job<'a> {
    pub model: &'a str,
    pub batch: &'a Path,
    pub logs: &'a Path,
    pub snapshots: &'a [PathBuf],
    pub prompt: &'a str,
}

pub fn run(agent: &Agent, job: &Job) -> Result<Outcome, String> {
    fs::create_dir_all(job.logs)
        .map_err(|e| format!("I cannot create {}: {e}", job.logs.display()))?;
    let events_path = job.logs.join(match agent.kind {
        Kind::Codex => "events.jsonl",
        Kind::Claude => "events.json",
    });
    let stderr_path = job.logs.join("stderr.txt");
    let response_path = job.logs.join("response.json");
    let mut command = Command::new(&agent.program);
    match agent.kind {
        Kind::Codex => {
            command
                .arg("exec")
                .arg("-s")
                .arg("read-only")
                .arg("-C")
                .arg(job.batch)
                .args(["--skip-git-repo-check", "--ephemeral", "-m", job.model])
                .args([
                    "-c",
                    "model_reasoning_effort=high",
                    "--json",
                    "--output-schema",
                ])
                .arg(job.batch.join("output-schema.json"))
                .arg("-o")
                .arg(&response_path)
                .arg(job.prompt);
        }
        Kind::Claude => {
            let schema = fs::read_to_string(job.batch.join("output-schema.json"))
                .map_err(|e| format!("I cannot read the output schema: {e}"))?;
            command
                .args(["-p", "--model", job.model, "--restricted"])
                .args([
                    "--tools",
                    "Read,Glob,Grep",
                    "--allowedTools",
                    "Read,Glob,Grep",
                ])
                .args(["--permission-mode", "dontAsk", "--strict-mcp-config"])
                .args([
                    "--mcp-config",
                    r#"{"mcpServers":{}}"#,
                    "--setting-sources",
                    "",
                ])
                .args(["--disable-slash-commands", "--no-session-persistence"])
                .args(["--output-format", "json", "--json-schema", &schema]);
            for snapshot in job.snapshots {
                command.arg("--add-dir").arg(snapshot);
            }
            command.arg("--").arg(job.prompt);
        }
    }
    let stdout = fs::File::create(&events_path).map_err(|e| e.to_string())?;
    let stderr = fs::File::create(&stderr_path).map_err(|e| e.to_string())?;
    let status = command
        .current_dir(job.batch)
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
        .status()
        .map_err(|e| format!("I cannot start {}: {e}", agent.program))?;
    let events = fs::read(&events_path).unwrap_or_default();
    let errors = fs::read_to_string(&stderr_path).unwrap_or_default();
    let all_output = format!("{}\n{errors}", String::from_utf8_lossy(&events));
    let events_sha256 = hex(&Sha256::digest(&events));
    match agent.kind {
        Kind::Codex => {
            if !status.success() {
                if let Some(text) = usage_limit(&all_output) {
                    return Ok(Outcome::UsageLimit(text));
                }
                return Err(failure(agent, &errors, job.logs));
            }
            let text = fs::read_to_string(&response_path).unwrap_or_default();
            let answer = serde_json::from_str(&text).map_err(|_| {
                "Codex gave no usable answer. Run the same command again.".to_string()
            })?;
            Ok(Outcome::Answer {
                answer,
                events_sha256,
                usage: codex_usage(&String::from_utf8_lossy(&events)),
            })
        }
        Kind::Claude => {
            let response: Value = serde_json::from_slice(&events).unwrap_or(Value::Null);
            let is_error = response
                .get("is_error")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if !status.success() || is_error || response.is_null() {
                if let Some(text) = usage_limit(&all_output) {
                    return Ok(Outcome::UsageLimit(text));
                }
                return Err(failure(agent, &all_output, job.logs));
            }
            let answer = match response.get("structured_output") {
                Some(value) if !value.is_null() => value.clone(),
                _ => response
                    .get("result")
                    .and_then(Value::as_str)
                    .and_then(|text| serde_json::from_str(text).ok())
                    .ok_or("Claude Code gave no usable answer. Run the same command again.")?,
            };
            Ok(Outcome::Answer {
                answer,
                events_sha256,
                usage: response.get("usage").cloned().unwrap_or(Value::Null),
            })
        }
    }
}

fn failure(agent: &Agent, output: &str, logs: &Path) -> String {
    let tail: Vec<&str> = output.lines().rev().take(5).collect();
    let tail: Vec<&str> = tail.into_iter().rev().collect();
    format!(
        "{} stopped with an error:\n{}\nRun the same command again. If it fails again, send the files in {} to the Flows maintainers.",
        agent.kind.name(),
        tail.join("\n"),
        logs.display()
    )
}

/// The token counts from the last `turn.completed` event of a Codex run.
pub fn codex_usage(events: &str) -> Value {
    events
        .lines()
        .rev()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|event| event.get("type").and_then(Value::as_str) == Some("turn.completed"))
        .filter_map(|event| event.get("usage").cloned())
        .next()
        .unwrap_or(Value::Null)
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_codex_usage() {
        let events = "{\"type\":\"turn.started\"}\n{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":5}}\n";
        assert_eq!(codex_usage(events)["input_tokens"], 5);
        assert!(codex_usage("not json").is_null());
    }
}
