//! Paths, server settings and the files that keep the session and consent.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const PLACEHOLDER_SERVER: &str = "https://flows-crowd.REPLACE.workers.dev";

/// The server for this build: `FLOWS_SERVER_URL` at build time, else the placeholder.
pub const DEFAULT_SERVER: &str = match option_env!("FLOWS_SERVER_URL") {
    Some(url) => url,
    None => PLACEHOLDER_SERVER,
};
pub const CLIENT_VERSION: u64 = 1;

pub fn server_url() -> Result<String, String> {
    let url = env_or("FLOWS_SERVER", DEFAULT_SERVER)
        .trim_end_matches('/')
        .to_string();
    if url == PLACEHOLDER_SERVER {
        return Err(
            "This build does not know the Flows server address. The maintainer must set \
                    FLOWS_SERVER_URL when building the client, or you can set FLOWS_SERVER."
                .to_string(),
        );
    }
    Ok(url)
}

pub fn github_url() -> String {
    env_or("FLOWS_GITHUB_URL", "https://github.com")
        .trim_end_matches('/')
        .to_string()
}

pub fn codeload_url() -> String {
    env_or("FLOWS_CODELOAD_URL", "https://codeload.github.com")
        .trim_end_matches('/')
        .to_string()
}

/// The GitHub OAuth App client id: the environment first, then the value set at build time.
pub fn github_client_id() -> Result<String, String> {
    if let Ok(value) = std::env::var("FLOWS_GITHUB_CLIENT_ID") {
        if !value.is_empty() {
            return Ok(value);
        }
    }
    match option_env!("FLOWS_GITHUB_CLIENT_ID") {
        Some(value) if !value.is_empty() => Ok(value.to_string()),
        _ => Err("This build has no GitHub OAuth App client id. The maintainer must set \
                  FLOWS_GITHUB_CLIENT_ID when building the client, or you can set it in the environment."
            .to_string()),
    }
}

fn env_or(name: &str, default: &str) -> String {
    match std::env::var(name) {
        Ok(value) if !value.is_empty() => value,
        _ => default.to_string(),
    }
}

pub fn home() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .ok_or_else(|| "I cannot find your home folder.".to_string())
}

pub fn config_dir() -> Result<PathBuf, String> {
    if let Some(value) = std::env::var_os("FLOWS_CONFIG") {
        return Ok(PathBuf::from(value));
    }
    if cfg!(windows) {
        if let Some(value) = std::env::var_os("APPDATA") {
            return Ok(PathBuf::from(value).join("flows-contribute"));
        }
    }
    if cfg!(target_os = "macos") {
        return Ok(home()?.join("Library/Application Support/flows-contribute"));
    }
    if let Some(value) = std::env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(value).join("flows-contribute"));
    }
    Ok(home()?.join(".config/flows-contribute"))
}

pub fn work_root() -> Result<PathBuf, String> {
    match std::env::var_os("FLOWS_WORK") {
        Some(value) => Ok(PathBuf::from(value)),
        None => Ok(home()?.join("FlowsWork")),
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Session {
    pub session_token: String,
    pub contributor_id: String,
    pub login: String,
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct State {
    #[serde(default)]
    pub consented_terms: Vec<String>,
}

pub fn session_path() -> Result<PathBuf, String> {
    Ok(config_dir()?.join("session.json"))
}

pub fn load_session() -> Result<Option<Session>, String> {
    read_json(&session_path()?)
}

pub fn save_session(session: &Session) -> Result<(), String> {
    write_private(
        &session_path()?,
        &serde_json::to_vec_pretty(session).unwrap(),
    )
}

pub fn remove_session() -> Result<bool, String> {
    let path = session_path()?;
    if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("I cannot remove {}: {e}", path.display()))?;
        return Ok(true);
    }
    Ok(false)
}

pub fn load_state() -> Result<State, String> {
    Ok(read_json(&config_dir()?.join("state.json"))?.unwrap_or_default())
}

pub fn save_state(state: &State) -> Result<(), String> {
    write_private(
        &config_dir()?.join("state.json"),
        &serde_json::to_vec_pretty(state).unwrap(),
    )
}

fn read_json<T: for<'a> Deserialize<'a>>(path: &Path) -> Result<Option<T>, String> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(|e| {
            format!(
                "The file {} is damaged ({e}). Delete it and try again.",
                path.display()
            )
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("I cannot read {}: {e}", path.display())),
    }
}

/// Write a file that only the current user can read (mode 0600 on Unix).
pub fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("I cannot create {}: {e}", parent.display()))?;
    }
    let temporary = path.with_extension("tmp");
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|e| format!("I cannot write {}: {e}", temporary.display()))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| format!("I cannot write {}: {e}", temporary.display()))?;
    fs::rename(&temporary, path).map_err(|e| format!("I cannot write {}: {e}", path.display()))
}
