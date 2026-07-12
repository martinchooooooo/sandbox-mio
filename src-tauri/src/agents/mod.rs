pub mod types;

use std::sync::Arc;

use tauri::{ipc::Channel, Emitter};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::process_ext::NoWindow;
use crate::runtime::types::RuntimeManager;
use types::{AgentAuthEvent, AgentAuthStatus, AgentError, AgentInstallEvent, AuthSession, AuthSessionMap};

static SESSION_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn new_session_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let count = SESSION_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("{ts}-{count}")
}

pub(crate) fn json_path_get<'a>(val: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    path.split('.').fold(Some(val), |acc, key| acc?.get(key))
}

pub async fn install_packages(
    name: &str,
    packages: &[String],
    on_progress: &Channel<serde_json::Value>,
    runtime: &Arc<dyn RuntimeManager>,
) -> Result<(), AgentError> {
    for package in packages {
        on_progress
            .send(serde_json::to_value(&AgentInstallEvent::PackageStart { package: package.clone() }).unwrap())
            .ok();
        runtime
            .install(name, package)
            .await
            .map_err(|e| AgentError::InstallFailed(e.to_string()))?;
        on_progress
            .send(serde_json::to_value(&AgentInstallEvent::PackageDone { package: package.clone() }).unwrap())
            .ok();
    }
    Ok(())
}

pub async fn run_postinstall_npm(
    name: &str,
    npm_package: &str,
    node_tool: &str,
    on_progress: &Channel<serde_json::Value>,
    runtime: &Arc<dyn RuntimeManager>,
) -> Result<(), AgentError> {
    let step = format!("npm-postinstall:{npm_package}");
    on_progress
        .send(serde_json::to_value(&AgentInstallEvent::PostInstallStart { step: step.clone() }).unwrap())
        .ok();

    let mise_tool = format!("npm:{npm_package}");
    let tool_dir = runtime
        .tool_dir(name, &mise_tool)
        .await
        .map_err(|e| AgentError::PostInstallFailed(e.to_string()))?;
    let tool_dir = tool_dir.trim().to_string();

    let install_script = format!("{tool_dir}/lib/node_modules/{npm_package}/install.cjs");
    runtime
        .exec_in_container(name, &["mise", "exec", node_tool, "--", "node", &install_script])
        .await
        .map_err(|e| AgentError::PostInstallFailed(e.to_string()))?;

    on_progress
        .send(serde_json::to_value(&AgentInstallEvent::PostInstallDone { step }).unwrap())
        .ok();

    Ok(())
}

async fn stream_output<R, F>(mut reader: R, mut emit: F)
where
    R: tokio::io::AsyncRead + Unpin,
    F: FnMut(String),
{
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match tokio::time::timeout(std::time::Duration::from_millis(80), reader.read(&mut chunk)).await {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => {
                buf.extend_from_slice(&chunk[..n]);
                while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                    let line: Vec<u8> = buf.drain(..=pos).collect();
                    emit(String::from_utf8_lossy(&line).trim_end_matches(['\r', '\n']).to_string());
                }
            }
            Ok(Err(_)) => break,
            Err(_) => {
                if !buf.is_empty() {
                    emit(String::from_utf8_lossy(&buf).to_string());
                    buf.clear();
                }
            }
        }
    }
    if !buf.is_empty() {
        emit(String::from_utf8_lossy(&buf).to_string());
    }
}

pub async fn start_auth(
    name: &str,
    auth_command: &[String],
    app: &tauri::AppHandle,
    sessions: &AuthSessionMap,
    runtime: &Arc<dyn RuntimeManager>,
) -> Result<String, AgentError> {
    let connection = runtime.local_connection().to_string();
    let session_id = new_session_id();

    tokio::process::Command::new("podman")
        .no_window()
        .args(["--connection", &connection, "start", name])
        .output()
        .await
        .map_err(|e| AgentError::AuthFailed(e.to_string()))?;

    let mut cmd = tokio::process::Command::new("podman");
    cmd.no_window();
    cmd.arg("--connection").arg(&connection);
    cmd.args(["exec", "-i", "--workdir", "/root/work", name, "mise", "exec", "--"]);
    for arg in auth_command {
        cmd.arg(arg);
    }
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| AgentError::AuthFailed(format!("spawn failed: {e}")))?;

    let stdin = child.stdin.take().ok_or_else(|| AgentError::AuthFailed("stdin not piped".into()))?;
    let stdout = child.stdout.take().ok_or_else(|| AgentError::AuthFailed("stdout not piped".into()))?;
    let stderr = child.stderr.take().ok_or_else(|| AgentError::AuthFailed("stderr not piped".into()))?;

    sessions.lock().await.insert(session_id.clone(), AuthSession { stdin });

    let event_name = format!("agent-auth-output-{session_id}");

    let app_out = app.clone();
    let ename_out = event_name.clone();
    let stdout_handle = tokio::spawn(async move {
        stream_output(stdout, move |line| {
            app_out.emit(&ename_out, &AgentAuthEvent::Output { line }).ok();
        })
        .await;
    });

    let app_err = app.clone();
    let ename_err = event_name.clone();
    let stderr_handle = tokio::spawn(async move {
        stream_output(stderr, move |line| {
            app_err.emit(&ename_err, &AgentAuthEvent::Output { line }).ok();
        })
        .await;
    });

    let app_wait = app.clone();
    let sessions_wait = sessions.clone();
    let session_id_wait = session_id.clone();
    tokio::spawn(async move {
        let code = child.wait().await.ok().and_then(|s| s.code()).unwrap_or(-1);
        let _ = tokio::join!(stdout_handle, stderr_handle);
        sessions_wait.lock().await.remove(&session_id_wait);
        app_wait.emit(&event_name, &AgentAuthEvent::Exit { code }).ok();
    });

    Ok(session_id)
}

pub async fn send_auth_input(
    session_id: &str,
    input: &str,
    sessions: &AuthSessionMap,
) -> Result<(), AgentError> {
    let session = sessions
        .lock()
        .await
        .remove(session_id)
        .ok_or_else(|| AgentError::AuthSessionNotFound(session_id.to_string()))?;
    let mut stdin = session.stdin;
    let line = format!("{input}\n");
    stdin
        .write_all(line.as_bytes())
        .await
        .map_err(|e| AgentError::AuthFailed(e.to_string()))?;
    Ok(())
}

pub async fn cancel_auth(session_id: &str, sessions: &AuthSessionMap) -> Result<(), AgentError> {
    sessions.lock().await.remove(session_id);
    Ok(())
}

pub async fn get_auth_status(
    name: &str,
    credential_path: &str,
    expires_at_json_key: &str,
    account_source_path: Option<&str>,
    account_email_json_key: Option<&str>,
    runtime: &Arc<dyn RuntimeManager>,
) -> Result<AgentAuthStatus, AgentError> {
    let cred_json = match runtime.exec_in_container(name, &["cat", credential_path]).await {
        Err(_) => {
            return Ok(AgentAuthStatus { authenticated: false, expires_at_ms: None, account_email: None })
        }
        Ok(s) => s,
    };

    let cred: serde_json::Value = serde_json::from_str(cred_json.trim())
        .map_err(|e| AgentError::CredentialReadFailed(e.to_string()))?;

    let expires_at_ms = json_path_get(&cred, expires_at_json_key).and_then(|v| v.as_u64());

    let account_email =
        if let (Some(src_path), Some(email_key)) = (account_source_path, account_email_json_key) {
            runtime
                .exec_in_container(name, &["cat", src_path])
                .await
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(s.trim()).ok())
                .and_then(|v| {
                    json_path_get(&v, email_key)
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                })
        } else {
            None
        };

    let authenticated = expires_at_ms.is_some();
    Ok(AgentAuthStatus { authenticated, expires_at_ms, account_email })
}
