//! The agent host subsystem: the host side of the native chat, which drives the pi supervisor
//! inside a sandbox through the engine's streamed exec
//! (`docs/features/native-agent-ui/02_dev.md`, Resolved Decision 1).

pub mod dialogs;
pub mod framing;
mod mock;
pub mod sessions;
pub mod types;

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::ipc::Channel;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin};
use tokio::sync::Mutex;

use crate::provider::types::{ExecOpts, MachineProvider, MachineRef};
use crate::provider::ProviderRegistry;
use types::{AgentHost, AgentHostError, AttachEvent, AttachInfo, Cursor, HostStatus};

const PI_HOST_SCRIPT: &str = include_str!("../../resources/agent-host/pi-host.sh");
const PI_ATTACH_SCRIPT: &str = include_str!("../../resources/agent-host/pi-attach.sh");
const SCRIPT_DIR: &str = "/usr/local/lib/mio";
/// `pi-host.sh ensure` exits with this when its own `SCRIPT_VERSION` differs from the argument.
const EXIT_VERSION_MISMATCH: i32 = 10;
/// `pi-host.sh ensure` exits with this when pi did not survive its start.
const EXIT_START_FAILED: i32 = 4;
/// The wrapper below exits with this when the script is not installed.
const EXIT_MISSING: i32 = 127;
/// Podman exec takes about 1.4 s to set up (spike 01); the header follows immediately after.
const HEADER_TIMEOUT: Duration = Duration::from_secs(30);

/// Constructs the agent host: the replay mock under `MIO_MOCK_BACKEND=1`, because a streamed exec
/// child bypasses the scripted `ProcessRunner` and would reach a real engine.
pub fn default_agent_host(registry: Arc<ProviderRegistry>) -> Arc<dyn AgentHost> {
    if crate::backends::mock_backend_enabled() {
        return Arc::new(mock::MockAgentHost::default());
    }
    Arc::new(ExecAgentHost { registry, attaches: Arc::default(), next_id: AtomicU64::new(1) })
}

/// `SCRIPT_VERSION` of the embedded `pi-host.sh`, the single source of the version the app checks.
fn script_version() -> &'static str {
    PI_HOST_SCRIPT
        .lines()
        .find_map(|l| l.strip_prefix("SCRIPT_VERSION="))
        .map(str::trim)
        .unwrap_or("0")
}

/// One live attach stream. Dropping it kills the exec client, which ends the in-sandbox attach.
struct LiveAttach {
    id: u64,
    writer: bool,
    stdin: ChildStdin,
    _child: Child,
}

type AttachMap = Arc<Mutex<HashMap<String, LiveAttach>>>;

struct ExecAgentHost {
    registry: Arc<ProviderRegistry>,
    attaches: AttachMap,
    next_id: AtomicU64,
}

impl ExecAgentHost {
    /// Runs `pi-host.sh <args>`; a missing script yields exit 127 on every shell, where `sh <file>`
    /// alone would exit 2 under dash.
    async fn run_host(
        &self,
        provider: &Arc<dyn MachineProvider>,
        name: &str,
        args: &[&str],
    ) -> Result<std::process::Output, AgentHostError> {
        let wrapper = format!(
            "test -f {SCRIPT_DIR}/pi-host.sh || exit {EXIT_MISSING}; exec sh {SCRIPT_DIR}/pi-host.sh \"$@\""
        );
        let mut argv = vec!["sh", "-c", wrapper.as_str(), "pi-host"];
        argv.extend_from_slice(args);
        Ok(provider.exec(name, &argv, ExecOpts::default()).await?)
    }

    async fn install(
        &self,
        provider: &Arc<dyn MachineProvider>,
        name: &str,
    ) -> Result<(), AgentHostError> {
        for (file, body) in [("pi-host.sh", PI_HOST_SCRIPT), ("pi-attach.sh", PI_ATTACH_SCRIPT)] {
            // A CRLF checkout of the resources would break `sh`; `.gitattributes` pins LF as well.
            let body = body.replace("\r\n", "\n");
            let script = format!(
                "mkdir -p {SCRIPT_DIR} && cat > {SCRIPT_DIR}/{file}.tmp && mv {SCRIPT_DIR}/{file}.tmp {SCRIPT_DIR}/{file}"
            );
            let out = provider
                .exec_piped(name, &["sh", "-c", &script], body.as_bytes())
                .await
                .map_err(|e| AgentHostError::ScriptInstall(e.to_string()))?;
            if !out.status.success() {
                return Err(AgentHostError::ScriptInstall(stderr_of(&out)));
            }
        }
        Ok(())
    }
}

fn stderr_of(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stderr).trim().to_string()
}

#[async_trait::async_trait]
impl AgentHost for ExecAgentHost {
    async fn ensure(&self, name: &str, machine: &MachineRef) -> Result<HostStatus, AgentHostError> {
        let provider = self.registry.provider_for(machine);
        let version = script_version();
        let mut out = self.run_host(&provider, name, &["ensure", version]).await?;
        if matches!(out.status.code(), Some(EXIT_VERSION_MISMATCH | EXIT_MISSING)) {
            self.install(&provider, name).await?;
            out = self.run_host(&provider, name, &["ensure", version]).await?;
        }
        match out.status.code() {
            Some(0) => dialogs::parse_ensure_output(&String::from_utf8_lossy(&out.stdout)),
            Some(EXIT_START_FAILED) => Err(AgentHostError::NotRunning(stderr_of(&out))),
            code => Err(AgentHostError::Provider(format!(
                "supervisor exited with {code:?}: {}",
                stderr_of(&out)
            ))),
        }
    }

    async fn attach(
        &self,
        name: &str,
        machine: &MachineRef,
        cursor: Option<Cursor>,
        on_event: Channel<AttachEvent>,
    ) -> Result<AttachInfo, AgentHostError> {
        // Replaced before spawning, so the old stream's writer lock is released for the new one.
        self.detach(name).await;
        let provider = self.registry.provider_for(machine);
        let (generation, offset) = match &cursor {
            Some(c) => (c.generation.clone(), c.offset.to_string()),
            None => ("-".to_string(), "end".to_string()),
        };
        let script = format!("{SCRIPT_DIR}/pi-attach.sh");
        let mut cmd = provider.exec_command(
            name,
            &["sh", &script, &generation, &offset],
            ExecOpts::default(),
        );
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true);
        let mut child = cmd.spawn().map_err(|e| AgentHostError::Stream(format!("spawn failed: {e}")))?;
        let stdin = child.stdin.take().ok_or_else(|| AgentHostError::Stream("stdin not piped".into()))?;
        let stdout = child.stdout.take().ok_or_else(|| AgentHostError::Stream("stdout not piped".into()))?;
        let mut reader = BufReader::new(stdout);

        let mut header = String::new();
        tokio::time::timeout(HEADER_TIMEOUT, reader.read_line(&mut header))
            .await
            .map_err(|_| AgentHostError::Stream("no attach header".into()))?
            .map_err(|e| AgentHostError::Stream(e.to_string()))?;
        let header: serde_json::Value = serde_json::from_str(&header)
            .map_err(|_| AgentHostError::NotRunning(format!("unexpected attach output: {header}")))?;
        if header["type"] != "mio_attach" {
            return Err(AgentHostError::NotRunning(header["error"].as_str().unwrap_or("").to_string()));
        }
        let info = AttachInfo {
            generation: header["generation"].as_str().unwrap_or_default().to_string(),
            offset: header["offset"].as_u64().unwrap_or(0),
            writer: header["writer"].as_bool().unwrap_or(false),
        };

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.attaches.lock().await.insert(
            name.to_string(),
            LiveAttach { id, writer: info.writer, stdin, _child: child },
        );
        tokio::spawn(read_stream(reader, info.offset, on_event, self.attaches.clone(), name.to_string(), id));
        Ok(info)
    }

    async fn send(&self, name: &str, line: String) -> Result<(), AgentHostError> {
        if line.contains('\n') {
            return Err(AgentHostError::Stream("a command must be a single line".into()));
        }
        let mut attaches = self.attaches.lock().await;
        let live = attaches.get_mut(name).ok_or_else(|| AgentHostError::Stream("not attached".into()))?;
        if !live.writer {
            return Err(AgentHostError::ReadOnly);
        }
        let mut bytes = line.into_bytes();
        bytes.push(b'\n');
        live.stdin.write_all(&bytes).await.map_err(|e| AgentHostError::Stream(e.to_string()))?;
        live.stdin.flush().await.map_err(|e| AgentHostError::Stream(e.to_string()))
    }

    async fn detach(&self, name: &str) {
        self.attaches.lock().await.remove(name);
    }

    async fn detach_all(&self) {
        self.attaches.lock().await.clear();
    }

    async fn takeover(&self, name: &str, machine: &MachineRef) -> Result<(), AgentHostError> {
        let provider = self.registry.provider_for(machine);
        let out = self.run_host(&provider, name, &["takeover"]).await?;
        if !out.status.success() {
            return Err(AgentHostError::Provider(format!("takeover failed: {}", stderr_of(&out))));
        }
        Ok(())
    }

    async fn stop(&self, name: &str, machine: &MachineRef) -> Result<(), AgentHostError> {
        self.detach(name).await;
        let provider = self.registry.provider_for(machine);
        let out = self.run_host(&provider, name, &["stop"]).await?;
        if !out.status.success() {
            return Err(AgentHostError::Provider(format!("stop failed: {}", stderr_of(&out))));
        }
        Ok(())
    }
}

/// Forwards every record to the frontend, then a `mio_detached` record, and drops the attach's
/// map entry unless a newer attach has replaced it.
async fn read_stream(
    mut reader: BufReader<tokio::process::ChildStdout>,
    mut offset: u64,
    on_event: Channel<AttachEvent>,
    attaches: AttachMap,
    name: String,
    id: u64,
) {
    let reason = loop {
        match framing::next_record(&mut reader, &mut offset).await {
            Ok(Some(framed)) => {
                if let Some(event) = framed.event {
                    if on_event.send(event).is_err() {
                        break "channel closed";
                    }
                }
            }
            Ok(None) => break "stream ended",
            Err(e) => {
                tracing::warn!(target: "agent_host", sandbox = %name, error = %e, "attach stream read failed");
                break "read error";
            }
        }
    };
    let _ = on_event.send(detached_event(reason));
    let mut map = attaches.lock().await;
    if map.get(&name).is_some_and(|a| a.id == id) {
        map.remove(&name);
    }
}

fn detached_event(reason: &str) -> AttachEvent {
    AttachEvent {
        offset: None,
        end: None,
        record: serde_json::json!({ "type": "mio_detached", "reason": reason }),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_script_version_is_read_from_the_embedded_supervisor() {
        assert!(super::script_version().parse::<u32>().is_ok());
        assert!(super::PI_ATTACH_SCRIPT.contains(&format!("SCRIPT_VERSION={}", super::script_version())));
    }
}
