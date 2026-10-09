//! The commands: login, run, status and logout.

use crate::agent::{self, Agent, Kind, Outcome};
use crate::api::{Reply, Server};
use crate::config::{self, Session};
use crate::{github, labels, snapshot};
use regex::Regex;
use serde_json::{json, Value};
use std::fs;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

/// A command ends with `Ok` (including a normal stop, such as a usage limit) or a problem text.
pub type CommandResult = Result<(), String>;

pub fn login() -> CommandResult {
    let client_id = config::github_client_id()?;
    println!("Log in with your GitHub account. Flows asks for no GitHub permissions.");
    let token = github::device_login(&config::github_url(), &client_id)?;
    let reply = Server::new(config::server_url()?, None)
        .post("/v1/session", &json!({ "github_token": token }))?;
    if reply.status != 200 {
        return Err(match reply.error_code() {
            "account_too_new" => format!(
                "Your GitHub account is too new to contribute. {}",
                reply.message()
            ),
            "suspended" => format!("Your contributor account is suspended. {}", reply.message()),
            _ => format!("The login failed. {}", reply.message()),
        });
    }
    let text = |key: &str| {
        reply
            .body
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let session = Session {
        session_token: text("session_token"),
        contributor_id: text("contributor_id"),
        login: text("login"),
    };
    if session.session_token.is_empty() {
        return Err("The server did not return a session.".to_string());
    }
    config::save_session(&session)?;
    println!(
        "You are logged in as {}. Next, run: flows-contribute run",
        session.login
    );
    Ok(())
}

pub fn logout() -> CommandResult {
    if config::remove_session()? {
        println!("You are logged out.");
    } else {
        println!("You were not logged in.");
    }
    Ok(())
}

fn session() -> Result<Session, String> {
    config::load_session()?
        .ok_or_else(|| "You are not logged in. Run: flows-contribute login".to_string())
}

pub fn status() -> CommandResult {
    let session = session()?;
    let reply = Server::new(config::server_url()?, Some(session.session_token)).get("/v1/me")?;
    if reply.status == 401 {
        return Err("Your login has expired. Run: flows-contribute login".to_string());
    }
    if reply.status != 200 {
        return Err(reply.message());
    }
    let body = &reply.body;
    println!(
        "Contributor: {}",
        body["login"].as_str().unwrap_or(&session.login)
    );
    println!("Status: {}", body["status"].as_str().unwrap_or("unknown"));
    println!("Labels sent: {}", body["labels"]);
    println!(
        "Batches today: {} of {}",
        body["batches_today"], body["daily_limit"]
    );
    Ok(())
}

pub struct RunOptions {
    pub agent: Option<Kind>,
    pub model: Option<String>,
    pub once: bool,
}

fn choose_agent(choice: Option<Kind>) -> Result<Agent, String> {
    let kinds = match choice {
        Some(kind) => vec![kind],
        None => vec![Kind::Codex, Kind::Claude],
    };
    for kind in &kinds {
        if let Some(found) = agent::find(*kind) {
            if found.kind == Kind::Codex && !agent::codex_logged_in(&found) {
                if choice.is_some() {
                    return Err("Codex is not logged in. Run: codex login".to_string());
                }
                continue;
            }
            return Ok(found);
        }
    }
    Err(match choice {
        Some(kind) => format!(
            "I cannot find '{}'. Install it first. See the README.",
            kind.name()
        ),
        None => {
            "I cannot find Codex or Claude Code. Install one of them and log in. See the README."
                .to_string()
        }
    })
}

fn ask_consent(server: &Server, terms: &Value) -> CommandResult {
    let version = terms["terms_version"].as_str().unwrap_or("").to_string();
    let mut state = config::load_state()?;
    if state.consented_terms.contains(&version) {
        return Ok(());
    }
    println!("\nTerms for contributors (version {version})\n");
    println!("{}", terms["text"].as_str().unwrap_or(""));
    println!(
        "\nYour labels are published under this license: {}",
        terms["license"].as_str().unwrap_or("")
    );
    print!("Type yes to agree: ");
    std::io::stdout().flush().ok();
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer).ok();
    if answer.trim().to_lowercase() != "yes" {
        return Err("You did not agree to the terms, so nothing was sent.".to_string());
    }
    let reply = server.post("/v1/consent", &json!({ "terms_version": version }))?;
    if reply.status != 204 && reply.status != 200 {
        return Err(format!(
            "The server did not record your agreement. {}",
            reply.message()
        ));
    }
    state.consented_terms.push(version);
    config::save_state(&state)
}

pub fn run(options: RunOptions) -> CommandResult {
    let session = session()?;
    let agent = choose_agent(options.agent)?;
    println!("Using {} ({}).", agent.kind.name(), agent.version);
    let server = Server::new(config::server_url()?, Some(session.session_token.clone()));
    let terms = server.get("/v1/terms")?;
    if terms.status != 200 {
        return Err(format!("I cannot read the terms. {}", terms.message()));
    }
    let terms = terms.body;
    if terms["license"].is_null() {
        println!("{}", terms["text"].as_str().unwrap_or(""));
        println!("\nUploads are not open yet. Please try again later.");
        return Ok(());
    }
    if terms["client_min_version"].as_u64().unwrap_or(0) > config::CLIENT_VERSION {
        return Err("This client is too old. Install the new version. See the README.".to_string());
    }
    let allowed: Vec<String> = terms["models"][agent.kind.name()]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|m| m.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let model = match options.model {
        Some(model) => model,
        None => allowed.first().cloned().ok_or_else(|| {
            format!(
                "Flows does not accept labels from {} at the moment.",
                agent.kind.name()
            )
        })?,
    };
    if !allowed.contains(&model) {
        return Err(format!(
            "Flows does not accept the model {model}. Use one of: {}",
            allowed.join(", ")
        ));
    }
    ask_consent(&server, &terms)?;
    let root = config::work_root()?;
    let mut consent_retried = false;
    loop {
        let reply = server.post(
            "/v1/work",
            &json!({"agent": agent.kind.name(), "model": model,
                     "client": {"version": config::CLIENT_VERSION, "os": std::env::consts::OS, "agent_version": agent.version}}),
        )?;
        match (reply.status, reply.error_code()) {
            (200, _) => {}
            (204, _) => {
                println!("There is no work right now. Thank you! Try again later.");
                return Ok(());
            }
            (401, _) => {
                return Err("Your login has expired. Run: flows-contribute login".to_string())
            }
            (403, _) => {
                return Err(format!(
                    "Your contributor account cannot take work. {}",
                    reply.message()
                ))
            }
            (409, "consent_required") if !consent_retried => {
                consent_retried = true;
                let mut state = config::load_state()?;
                state.consented_terms.clear();
                config::save_state(&state)?;
                let fresh = server.get("/v1/terms")?.body;
                ask_consent(&server, &fresh)?;
                continue;
            }
            (409, "uploads_closed") => {
                println!("Uploads are not open at the moment. Please try again later.");
                return Ok(());
            }
            (426, _) => {
                return Err(
                    "This client is too old. Install the new version. See the README.".to_string(),
                )
            }
            (429, _) => {
                println!("You reached today's limit of batches. Thank you! Run the same command again tomorrow.");
                return Ok(());
            }
            _ => return Err(format!("The server did not give work. {}", reply.message())),
        }
        match process(&server, &agent, &model, &root, &reply)? {
            Batch::Done(count) => println!("Batch done: {count} labels sent. Thank you!"),
            Batch::Expired => println!("That batch expired before upload. Getting a new one."),
            Batch::UsageLimit(text) => {
                println!(
                    "\nYour {} usage limit is used up for now. Nothing is lost.",
                    agent.kind.name()
                );
                if !text.is_empty() {
                    println!("{text}");
                }
                println!("Run the same command again later to continue.");
                return Ok(());
            }
        }
        if options.once {
            return Ok(());
        }
    }
}

enum Batch {
    Done(usize),
    Expired,
    UsageLimit(String),
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

fn process(
    server: &Server,
    agent: &Agent,
    model: &str,
    root: &Path,
    reply: &Reply,
) -> Result<Batch, String> {
    let work = &reply.body;
    let assignment = text(work, "assignment_id").to_string();
    if !Regex::new(r"^[A-Za-z0-9-]{1,64}$")
        .unwrap()
        .is_match(&assignment)
    {
        return Err("The server sent a bad assignment id.".to_string());
    }
    let units = work["units"].as_array().cloned().unwrap_or_default();
    let ids: Vec<String> = units
        .iter()
        .map(|unit| text(unit, "unit_id").to_string())
        .collect();
    println!(
        "\nBatch {assignment}: {} units in round {}.",
        units.len(),
        text(work, "round_id")
    );
    let snapshots_root = root.join("snapshots");
    let mut trees = std::collections::BTreeMap::new();
    for entry in work["snapshots"].as_array().cloned().unwrap_or_default() {
        let dir = text(&entry, "dir").to_string();
        let tree = snapshot::ensure(
            &config::codeload_url(),
            &snapshots_root,
            text(&entry, "repository"),
            text(&entry, "commit"),
            &dir,
        )?;
        trees.insert(dir, tree);
    }
    let batch = root.join("batches").join(&assignment);
    let logs = root.join("logs").join(&assignment);
    fs::create_dir_all(&batch).map_err(|e| format!("I cannot create {}: {e}", batch.display()))?;
    for name in ["codebook.md", "prompt.md", "output-schema.json"] {
        let content = work["files"][name]
            .as_str()
            .ok_or_else(|| format!("The server did not send {name}."))?;
        fs::write(batch.join(name), content).map_err(|e| e.to_string())?;
    }
    let mut lines = String::new();
    for unit in &units {
        let mut unit = unit.clone();
        let tree = trees
            .get(text(&unit, "snapshot"))
            .ok_or("The server sent a unit without its repository.")?;
        unit["snapshot"] = Value::String(tree.to_string_lossy().into_owned());
        lines.push_str(&serde_json::to_string(&unit).unwrap());
        lines.push('\n');
    }
    fs::write(batch.join("batch.jsonl"), lines).map_err(|e| e.to_string())?;

    let saved = logs.join("labels.json");
    let (answer, events_sha256, usage) = match fs::read(&saved)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
    {
        Some(previous) => {
            println!("  Reusing the answer from the last run.");
            (
                previous["answer"].clone(),
                previous["events_sha256"].clone(),
                previous["usage"].clone(),
            )
        }
        None => {
            let prompt = format!(
                "You are the independent contributor labeler in round {round}. Read prompt.md and codebook.md fully, \
                 then label every unit in batch.jsonl in order. The snapshot paths in batch.jsonl are the repository trees. \
                 Read only those files and the named snapshot trees. Do not read output-schema.json, any other folder or \
                 any other file on this computer. Return the labels object required by the response schema. Do not write \
                 files or run source code. You have {count} units.",
                round = text(work, "round_id"),
                count = units.len()
            );
            println!(
                "  {} is labeling. This can take 10 to 30 minutes. You can leave it running.",
                agent.kind.name()
            );
            let snapshot_dirs: Vec<PathBuf> = trees.values().cloned().collect();
            let job = agent::Job {
                model,
                batch: &batch,
                logs: &logs,
                snapshots: &snapshot_dirs,
                prompt: &prompt,
            };
            match agent::run(agent, &job)? {
                Outcome::UsageLimit(text) => return Ok(Batch::UsageLimit(text)),
                Outcome::Answer {
                    answer,
                    events_sha256,
                    usage,
                } => {
                    let record =
                        json!({"answer": answer, "events_sha256": events_sha256, "usage": usage});
                    fs::write(&saved, serde_json::to_vec_pretty(&record).unwrap())
                        .map_err(|e| e.to_string())?;
                    (answer, Value::String(events_sha256), usage)
                }
            }
        }
    };
    let mut checked = match labels::validate(&answer, &ids) {
        Ok(checked) => checked,
        Err(problem) => {
            let _ = fs::rename(&saved, logs.join("labels.rejected.json"));
            return Err(format!(
                "The agent's answer is not usable ({problem}). Run the same command again."
            ));
        }
    };
    let removed = labels::scrub(&mut checked);
    if removed > 0 {
        println!("  I removed {removed} pieces of text that looked like a password or key.");
    }
    let body = json!({
        "assignment_id": assignment,
        "labels": checked,
        "usage": if usage.is_object() { usage } else { Value::Null },
        "events_sha256": events_sha256,
        "client": {"version": config::CLIENT_VERSION, "os": std::env::consts::OS,
                   "agent": agent.kind.name(), "agent_version": agent.version},
    });
    let reply = server.post("/v1/results", &body)?;
    match (reply.status, reply.error_code()) {
        (200, _) => {
            fs::write(logs.join("uploaded"), b"").ok();
            fs::remove_dir_all(&batch).ok();
            Ok(Batch::Done(
                reply.body["received"]
                    .as_u64()
                    .unwrap_or(checked.len() as u64) as usize,
            ))
        }
        (404, _) => {
            fs::remove_dir_all(&batch).ok();
            let _ = fs::rename(&saved, logs.join("labels.expired.json"));
            Ok(Batch::Expired)
        }
        (422, _) => {
            let _ = fs::rename(&saved, logs.join("labels.rejected.json"));
            Err(format!(
                "The server refused this batch: {} Run the same command again to label it again.",
                reply.message()
            ))
        }
        (401, _) => Err(
            "Your login has expired. Run: flows-contribute login. Your answer is kept.".to_string(),
        ),
        _ => Err(format!(
            "The upload failed. {} Your answer is kept. Run the same command again.",
            reply.message()
        )),
    }
}
