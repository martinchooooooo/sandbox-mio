//! The terminal host subsystem: the native terminal's host side, which carries an SSH session over
//! the engine's streamed exec and attaches it to the sandbox's zellij session
//! (`docs/features/native-terminal/02_dev.md`).

mod mock;
mod ssh;
pub mod types;

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use russh::client::{Handle, Msg};
use russh::{Channel as SshChannel, ChannelMsg, Disconnect};
use tauri::ipc::{Channel, InvokeResponseBody};
use tokio::process::Child;
use tokio::sync::{mpsc, Mutex};

use crate::provider::types::{ExecOpts, MachineProvider};
use crate::provider::ProviderRegistry;
use types::{AttachRequest, TerminalError, TerminalEvent, TerminalHost};

/// The working directory of every pane a new zellij session opens.
const WORKDIR: &str = "/root/work";
/// Bounds the exec spawn, handshake and session creation together; a remote Podman exec is the
/// slowest of them.
const ATTACH_TIMEOUT: Duration = Duration::from_secs(60);
/// Input and resize commands queued per session before a sender waits.
const COMMAND_QUEUE: usize = 256;

/// Constructs the terminal host: the echo mock under `MIO_MOCK_BACKEND=1`, because the session's
/// exec child bypasses the scripted `ProcessRunner` and would reach a real engine.
pub fn default_terminal_host(registry: Arc<ProviderRegistry>) -> Arc<dyn TerminalHost> {
    if crate::backends::mock_backend_enabled() {
        return Arc::new(mock::MockTerminalHost::default());
    }
    Arc::new(SshTerminalHost { registry, sessions: Arc::default(), next_id: AtomicU64::new(1) })
}

/// The in-sandbox commands of one attach: the engine-exec argv that creates or resurrects the
/// zellij session, and the client command run over SSH. The session is never created over SSH,
/// because `sshd` drops the container's environment
/// (`docs/features/native-terminal/02_dev.md`, Resolved Decision 7).
fn session_commands(name: &str) -> (Vec<String>, String) {
    let create = ["zellij", "attach", "--create-background", name].map(String::from).to_vec();
    (create, format!("zellij attach {name}"))
}

/// Whether a failed `--create-background` failed only because the session is already running.
/// zellij 0.44 exits non-zero with "Session already exists" in that case, which for an attach is
/// success: the session the client attaches to is there.
fn session_already_running(stderr: &str) -> bool {
    stderr.to_ascii_lowercase().contains("session already exists")
}

enum SessionCommand {
    Input(Vec<u8>),
    Resize(u32, u32),
}

/// One live session's handle. Dropping it closes the command queue, which ends the session task.
struct LiveSession {
    id: u64,
    commands: mpsc::Sender<SessionCommand>,
}

type SessionMap = Arc<Mutex<HashMap<String, LiveSession>>>;

struct SshTerminalHost {
    registry: Arc<ProviderRegistry>,
    sessions: SessionMap,
    next_id: AtomicU64,
}

/// Everything the session task owns. The exec child is killed when the task drops it.
struct Session {
    handle: Handle<ssh::Client>,
    channel: SshChannel<Msg>,
    child: Child,
}

impl SshTerminalHost {
    async fn create_session(&self, provider: &Arc<dyn MachineProvider>, name: &str) -> Result<(), TerminalError> {
        let (argv, _) = session_commands(name);
        let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
        let opts = ExecOpts { workdir: Some(WORKDIR.to_string()), ..Default::default() };
        let out = provider.exec(name, &argv, opts).await?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            if session_already_running(&stderr) {
                return Ok(());
            }
            return Err(TerminalError::SessionCreate(stderr));
        }
        Ok(())
    }

    async fn open(&self, req: &AttachRequest) -> Result<Session, TerminalError> {
        let provider = self.registry.provider_for(&req.machine);
        if req.start {
            provider.start(&req.name).await?;
        }
        let mut cmd = provider.exec_command(&req.name, &["mio-sshd"], ExecOpts::default());
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true);
        let mut child = cmd.spawn().map_err(|e| TerminalError::Spawn(e.to_string()))?;
        let pipe = ssh::Pipe {
            r: child.stdout.take().ok_or_else(|| TerminalError::Spawn("stdout not piped".into()))?,
            w: child.stdin.take().ok_or_else(|| TerminalError::Spawn("stdin not piped".into()))?,
        };
        let handshake = ssh::connect(
            pipe,
            &req.name,
            &req.identity_path,
            &req.known_hosts_path,
            req.cols,
            req.rows,
        );
        let ((handle, channel), ()) = tokio::try_join!(handshake, self.create_session(&provider, &req.name))?;
        let (_, client) = session_commands(&req.name);
        channel.exec(true, client).await.map_err(|e| TerminalError::Handshake(e.to_string()))?;
        Ok(Session { handle, channel, child })
    }

    async fn live_sender(&self, name: &str) -> Result<mpsc::Sender<SessionCommand>, TerminalError> {
        self.sessions.lock().await.get(name).map(|s| s.commands.clone()).ok_or(TerminalError::NotAttached)
    }
}

#[async_trait::async_trait]
impl TerminalHost for SshTerminalHost {
    async fn attach(
        &self,
        req: AttachRequest,
        on_output: Channel<InvokeResponseBody>,
        on_event: Channel<TerminalEvent>,
    ) -> Result<(), TerminalError> {
        self.detach(&req.name).await;
        let session = tokio::time::timeout(ATTACH_TIMEOUT, self.open(&req))
            .await
            .map_err(|_| TerminalError::Handshake("timed out connecting to the sandbox".into()))??;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel(COMMAND_QUEUE);
        self.sessions.lock().await.insert(req.name.clone(), LiveSession { id, commands: tx });
        tokio::spawn(run_session(session, rx, on_output, on_event, self.sessions.clone(), req.name, id));
        Ok(())
    }

    async fn input(&self, name: &str, bytes: Vec<u8>) -> Result<(), TerminalError> {
        let sender = self.live_sender(name).await?;
        sender.send(SessionCommand::Input(bytes)).await.map_err(|_| TerminalError::NotAttached)
    }

    async fn resize(&self, name: &str, cols: u32, rows: u32) -> Result<(), TerminalError> {
        let sender = self.live_sender(name).await?;
        sender.send(SessionCommand::Resize(cols, rows)).await.map_err(|_| TerminalError::NotAttached)
    }

    async fn detach(&self, name: &str) {
        self.sessions.lock().await.remove(name);
    }

    async fn detach_all(&self) {
        self.sessions.lock().await.clear();
    }
}

/// Pumps output to the frontend and commands to the channel until either side ends, then reports
/// the exit and drops the map entry unless a newer session has replaced it.
async fn run_session(
    session: Session,
    mut commands: mpsc::Receiver<SessionCommand>,
    on_output: Channel<InvokeResponseBody>,
    on_event: Channel<TerminalEvent>,
    sessions: SessionMap,
    name: String,
    id: u64,
) {
    let Session { handle, channel, child } = session;
    let (mut reader, writer) = channel.split();
    let (reason, exit_status) = loop {
        tokio::select! {
            msg = reader.wait() => match msg {
                Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => {
                    if on_output.send(InvokeResponseBody::Raw(data.to_vec())).is_err() {
                        break ("window closed", None);
                    }
                }
                Some(ChannelMsg::ExitStatus { exit_status }) => break ("exited", Some(exit_status)),
                Some(ChannelMsg::Failure) => break ("the session command was refused", None),
                // `sshd` sends end-of-file before the exit status, so only a close ends the loop.
                Some(ChannelMsg::Close) | None => break ("connection closed", None),
                Some(_) => {}
            },
            cmd = commands.recv() => match cmd {
                Some(SessionCommand::Input(bytes)) => {
                    if writer.data_bytes(bytes).await.is_err() {
                        break ("connection closed", None);
                    }
                }
                Some(SessionCommand::Resize(cols, rows)) => {
                    if writer.window_change(cols, rows, 0, 0).await.is_err() {
                        break ("connection closed", None);
                    }
                }
                None => break ("detached", None),
            },
        }
    };
    tracing::info!(target: "terminal", sandbox = %name, reason, ?exit_status, "terminal session ended");
    let _ = on_event.send(TerminalEvent::Exited { reason: reason.to_string(), exit_status });
    let _ = handle.disconnect(Disconnect::ByApplication, "", "en").await;
    drop(child);
    let mut map = sessions.lock().await;
    if map.get(&name).is_some_and(|s| s.id == id) {
        map.remove(&name);
    }
}

#[cfg(test)]
mod tests {
    use super::{session_already_running, session_commands};

    #[test]
    fn a_running_session_is_not_a_create_failure() {
        assert!(session_already_running("Session already exists"));
        assert!(session_already_running("Error: session already exists: demo"));
        assert!(!session_already_running("No such file or directory"));
        assert!(!session_already_running(""));
    }

    #[test]
    fn the_session_is_created_by_exec_and_only_attached_over_ssh() {
        let (create, client) = session_commands("demo");
        assert_eq!(create, ["zellij", "attach", "--create-background", "demo"]);
        assert_eq!(client, "zellij attach demo");
    }
}
