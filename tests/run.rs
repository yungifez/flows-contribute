//! End-to-end test of `run --once` against a mock work queue, a mock codeload and a fake Codex.
#![cfg(unix)]

use flate2::write::GzEncoder;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

type Log = Arc<Mutex<Vec<(String, String, Value)>>>;

fn tarball() -> Vec<u8> {
    let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), flate2::Compression::fast()));
    let top = format!("name-{COMMIT}");
    let data = b"<?php class A {}";
    let mut header = tar::Header::new_gnu();
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    builder
        .append_data(&mut header, format!("{top}/app/a.php"), &data[..])
        .unwrap();
    let mut link = tar::Header::new_gnu();
    link.set_entry_type(tar::EntryType::Symlink);
    link.set_size(0);
    builder
        .append_link(&mut link, format!("{top}/link"), "/etc/passwd")
        .unwrap();
    builder.into_inner().unwrap().finish().unwrap()
}

fn work() -> Value {
    json!({
        "assignment_id": "a1b2c3", "round_id": "crowd-test", "expires_at": "2099-01-01T00:00:00Z",
        "codebook_version": "v1.1", "codebook_sha256": "00", "prompt_version": "model-label-v3",
        "files": {"codebook.md": "# Codebook", "prompt.md": "# Prompt", "output-schema.json": "{}"},
        "snapshots": [{"repository": "owner/name", "commit": COMMIT, "dir": "owner__name-0123456789ab"}],
        "units": [{"unit_id": "u1", "repository": "owner/name", "ecosystem": "laravel", "identifier": "POST x",
                   "method": "POST", "action": null, "relative_path": "routes/web.php", "line": 3,
                   "declaration": "Route::post(...)", "snapshot": "owner__name-0123456789ab"}]
    })
}

fn respond(stream: &mut TcpStream, status: u16, content_type: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {status} X\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).unwrap();
    stream.write_all(body).unwrap();
}

fn handle(mut stream: TcpStream, log: &Log, tar: &[u8]) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let mut parts = line.split_whitespace();
    let (method, path) = (
        parts.next().unwrap_or("").to_string(),
        parts.next().unwrap_or("").to_string(),
    );
    let mut length = 0;
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).unwrap();
        if header.trim().is_empty() {
            break;
        }
        if let Some(value) = header.to_lowercase().strip_prefix("content-length:") {
            length = value.trim().parse().unwrap();
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let uploaded = log
        .lock()
        .unwrap()
        .iter()
        .any(|(_, p, _)| p == "/v1/results");
    log.lock()
        .unwrap()
        .push((method.clone(), path.clone(), body));
    let json_reply = |stream: &mut TcpStream, status, value: Value| {
        respond(
            stream,
            status,
            "application/json",
            value.to_string().as_bytes(),
        )
    };
    match (method.as_str(), path.as_str()) {
        ("GET", "/v1/terms") => json_reply(
            &mut stream,
            200,
            json!({
            "terms_version": "t1", "license": "CC0-1.0", "text": "Test terms.",
            "models": {"codex": ["gpt-test"], "claude": []}, "client_min_version": 1}),
        ),
        ("POST", "/v1/consent") => respond(&mut stream, 204, "application/json", b""),
        ("POST", "/v1/work") if !uploaded => json_reply(&mut stream, 200, work()),
        ("POST", "/v1/work") => respond(&mut stream, 204, "application/json", b""),
        ("POST", "/v1/results") => json_reply(&mut stream, 200, json!({"received": 1})),
        ("GET", p) if p == format!("/owner/name/tar.gz/{COMMIT}") => {
            respond(&mut stream, 200, "application/gzip", tar)
        }
        _ => json_reply(
            &mut stream,
            404,
            json!({"error": "not_found", "message": "no route"}),
        ),
    }
}

fn serve() -> (String, Log) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let shared = log.clone();
    let tar = tarball();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            handle(stream.unwrap(), &shared, &tar);
        }
    });
    (url, log)
}

const FAKE_CODEX: &str = r#"#!/bin/sh
case "$1" in
  --version) echo "codex-cli 9.9.9-fake"; exit 0;;
  login) exit 0;;
  exec) shift;;
  *) exit 2;;
esac
if [ -f "$FAKE_LIMIT" ]; then echo "You've hit your usage limit. Try again at 4:05 PM." >&2; exit 1; fi
while [ $# -gt 0 ]; do
  case "$1" in -C) dir=$2; shift;; -o) out=$2; shift;; esac
  shift
done
unit=$(grep -o '"unit_id":"[^"]*"' "$dir/batch.jsonl" | head -1 | cut -d'"' -f4)
snap=$(grep -o '"snapshot":"[^"]*"' "$dir/batch.jsonl" | head -1 | cut -d'"' -f4)
[ -f "$snap/app/a.php" ] || { echo "missing snapshot" >&2; exit 3; }
if [ -e "$snap/link" ]; then echo "symlink kept" >&2; exit 4; fi
[ -f "$dir/codebook.md" ] || exit 5
printf '{"labels":[{"unit_id":"%s","disposition":"MATCH","flow_id":"resource.update","note":"saw AKIAABCDEFGHIJKLMNOP","handler":"A@b","files_read":["app/a.php"],"behavior_summary":"Updates A."}]}' "$unit" > "$out"
echo '{"type":"turn.completed","usage":{"input_tokens":7,"output_tokens":3}}'
"#;

fn client(dir: &Path, url: &str, stdin: &str) -> (bool, String) {
    let path = format!(
        "{}:{}",
        dir.join("bin").display(),
        std::env::var("PATH").unwrap()
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_flows-contribute"))
        .args(["run", "--agent", "codex", "--once"])
        .env("PATH", path)
        .env("FLOWS_SERVER", url)
        .env("FLOWS_CODELOAD_URL", url)
        .env("FLOWS_CONFIG", dir.join("config"))
        .env("FLOWS_WORK", dir.join("work"))
        .env("FAKE_LIMIT", dir.join("limit"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.success(), text)
}

#[test]
fn labels_one_batch_and_resumes_after_a_usage_limit() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join("codex"), FAKE_CODEX).unwrap();
    std::fs::set_permissions(bin.join("codex"), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::create_dir_all(dir.path().join("config")).unwrap();
    std::fs::write(
        dir.path().join("config/session.json"),
        r#"{"session_token":"s","contributor_id":"gh-1","login":"tester"}"#,
    )
    .unwrap();
    let (url, log) = serve();

    // The first run stops at the usage limit and sends nothing.
    std::fs::write(dir.path().join("limit"), "").unwrap();
    let (ok, text) = client(dir.path(), &url, "yes\n");
    assert!(ok, "{text}");
    assert!(text.contains("Try again at 4:05 PM."), "{text}");
    assert!(!log
        .lock()
        .unwrap()
        .iter()
        .any(|(_, p, _)| p == "/v1/results"));
    assert!(log
        .lock()
        .unwrap()
        .iter()
        .any(|(_, p, _)| p == "/v1/consent"));

    // The second run gets the same batch again, labels it and uploads.
    std::fs::remove_file(dir.path().join("limit")).unwrap();
    let (ok, text) = client(dir.path(), &url, "");
    assert!(ok, "{text}");
    assert!(text.contains("Batch done: 1 labels sent"), "{text}");
    let entries = log.lock().unwrap();
    assert_eq!(
        entries
            .iter()
            .filter(|(_, p, _)| p == "/v1/consent")
            .count(),
        1
    );
    let (_, _, body) = entries.iter().find(|(_, p, _)| p == "/v1/results").unwrap();
    assert_eq!(body["assignment_id"], "a1b2c3");
    assert_eq!(body["labels"][0]["unit_id"], "u1");
    assert_eq!(
        body["labels"][0]["note"],
        "[removed by client: possible secret]"
    );
    assert_eq!(body["usage"]["input_tokens"], 7);
    assert_eq!(body["events_sha256"].as_str().unwrap().len(), 64);
    assert_eq!(body["client"]["agent"], "codex");
    let tree = dir.path().join("work/snapshots/owner__name-0123456789ab");
    assert!(tree.join("app/a.php").is_file());
    assert!(std::fs::symlink_metadata(tree.join("link")).is_err());
    assert!(!dir.path().join("work/batches/a1b2c3").exists());
}
