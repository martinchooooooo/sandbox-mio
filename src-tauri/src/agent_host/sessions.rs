//! Host-side listing of the native chat's pi session files, readable while the sandbox is stopped.

use std::io::BufRead;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// The native chat's session directory, relative to the workspace. Mirrors `CHAT_SESSION_DIR` in
/// `src-tauri/resources/agent-host/pi-host.sh`; keep the two equal.
pub const CHAT_SESSION_DIR: &str = ".mio/pi-agent/chat-sessions";
/// Where the workspace is mounted inside every sandbox.
const SANDBOX_WORKSPACE: &str = "/root/work";

/// One session file, summarised for the session list.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChatSession {
    pub id: String,
    /// The path inside the sandbox, which `switch_session` needs; the host path would not resolve
    /// there (`docs/features/native-agent-ui/02_dev.md`, Common Gotchas).
    pub sandbox_path: String,
    /// The latest `session_info` name, if the session was named.
    pub name: Option<String>,
    /// The first user message's text, for sessions without a name.
    pub first_message: Option<String>,
    pub message_count: u32,
    pub modified_ms: u64,
}

/// Every session under the workspace's chat session directory, newest first. Unreadable or
/// headerless files are skipped.
pub fn list_sessions(workspace_path: &Path) -> Vec<ChatSession> {
    let root = workspace_path.join(CHAT_SESSION_DIR);
    let mut files = Vec::new();
    collect_jsonl(&root, &mut files);
    let mut sessions: Vec<ChatSession> =
        files.iter().filter_map(|f| summarise(&root, f)).collect();
    sessions.sort_by(|a, b| b.modified_ms.cmp(&a.modified_ms));
    sessions
}

fn collect_jsonl(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl(&path, out);
        } else if path.extension().is_some_and(|e| e == "jsonl") {
            out.push(path);
        }
    }
}

fn summarise(root: &Path, file: &Path) -> Option<ChatSession> {
    let reader = std::io::BufReader::new(std::fs::File::open(file).ok()?);
    let modified_ms = std::fs::metadata(file)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis() as u64);
    let relative = file.strip_prefix(root).ok()?;
    let mut summary = parse_session(reader.lines().map_while(Result::ok))?;
    summary.modified_ms = modified_ms;
    summary.sandbox_path = format!(
        "{SANDBOX_WORKSPACE}/{CHAT_SESSION_DIR}/{}",
        relative.to_string_lossy().replace('\\', "/")
    );
    Some(summary)
}

/// Summarises one session file's lines; `None` when the first line is not a `session` header.
fn parse_session(mut lines: impl Iterator<Item = String>) -> Option<ChatSession> {
    let header: serde_json::Value = serde_json::from_str(&lines.next()?).ok()?;
    if header["type"] != "session" {
        return None;
    }
    let mut session = ChatSession {
        id: header["id"].as_str()?.to_string(),
        sandbox_path: String::new(),
        name: None,
        first_message: None,
        message_count: 0,
        modified_ms: 0,
    };
    for line in lines {
        let Ok(entry) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
        match entry["type"].as_str() {
            Some("session_info") => {
                if let Some(name) = entry["name"].as_str() {
                    session.name = Some(name.to_string());
                }
            }
            Some("message") => {
                let message = &entry["message"];
                if !matches!(message["role"].as_str(), Some("user" | "assistant")) {
                    continue;
                }
                session.message_count += 1;
                if session.first_message.is_none() && message["role"] == "user" {
                    session.first_message = message_text(&message["content"]);
                }
            }
            _ => {}
        }
    }
    Some(session)
}

/// A message's text: either a plain string or the concatenated `text` parts.
fn message_text(content: &serde_json::Value) -> Option<String> {
    let text = match content {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(parts) => parts
            .iter()
            .filter(|p| p["type"] == "text")
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join(" "),
        _ => return None,
    };
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.chars().take(200).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION: &str = r#"{"type":"session","version":3,"id":"s-1","timestamp":"2026-11-10T21:01:02.285Z","cwd":"/root/work"}
{"type":"model_change","id":"a","parentId":null}
{"type":"message","id":"b","parentId":"a","message":{"role":"system","content":"sys"}}
{"type":"message","id":"c","parentId":"b","message":{"role":"user","content":[{"type":"text","text":"Explain named pipes"}]}}
{"type":"message","id":"d","parentId":"c","message":{"role":"assistant","content":[{"type":"text","text":"A FIFO."}]}}
{"type":"message","id":"e","parentId":"d","message":{"role":"toolResult","content":[]}}
{"type":"session_info","id":"f","parentId":"e","name":"Pipes"}
not json"#;

    #[test]
    fn a_session_file_is_summarised() {
        let s = parse_session(SESSION.lines().map(str::to_string)).unwrap();
        assert_eq!(s.id, "s-1");
        assert_eq!(s.name.as_deref(), Some("Pipes"));
        assert_eq!(s.first_message.as_deref(), Some("Explain named pipes"));
        assert_eq!(s.message_count, 2);
    }

    #[test]
    fn a_file_without_a_header_is_skipped() {
        assert!(parse_session(SESSION.lines().skip(1).map(str::to_string)).is_none());
    }

    #[test]
    fn listing_reports_the_sandbox_path_newest_first() {
        let dir = std::env::temp_dir().join(format!("mio-chat-sessions-{}", uuid::Uuid::new_v4()));
        let sub = dir.join(CHAT_SESSION_DIR).join("--root-work--");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("one.jsonl"), SESSION).unwrap();
        std::fs::write(sub.join("ignored.txt"), SESSION).unwrap();
        let sessions = list_sessions(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(
            sessions[0].sandbox_path,
            "/root/work/.mio/pi-agent/chat-sessions/--root-work--/one.jsonl"
        );
    }
}
