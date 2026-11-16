//! The terminal host subsystem: the native terminal's host side, which carries an SSH session over
//! the engine's streamed exec and attaches it to the sandbox's zellij session
//! (`docs/features/native-terminal/02_dev.md`).

mod mock;
mod sessions;
mod ssh;
pub mod types;

use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use russh::client::{Handle, Msg};
use russh::{Channel as SshChannel, ChannelMsg, Disconnect};
use tauri::ipc::{Channel, InvokeResponseBody};
use tokio::process::Child;
use tokio::sync::{mpsc, Mutex};

use crate::provider::types::{ExecOpts, MachineProvider};
use crate::provider::ProviderRegistry;
use sessions::SessionTable;
use types::{AttachRequest, TerminalError, TerminalEvent, TerminalHost};

/// The working directory of every pane a new zellij session opens.
const WORKDIR: &str = "/root/work";
/// Bounds the exec spawn, handshake and session creation together; a remote Podman exec is the
/// slowest of them.
const ATTACH_TIMEOUT: Duration = Duration::from_secs(60);
/// Total time `detach_all` waits for session tasks to end on app exit.
const EXIT_TIMEOUT: Duration = Duration::from_secs(3);
/// Input and resize commands queued per session before a sender waits.
const COMMAND_QUEUE: usize = 256;

/// Constructs the terminal host: the echo mock under `MIO_MOCK_BACKEND=1`, because the session's
/// exec child bypasses the scripted `ProcessRunner` and would reach a real engine.
pub fn default_terminal_host(registry: Arc<ProviderRegistry>) -> Arc<dyn TerminalHost> {
    if crate::backends::mock_backend_enabled() {
        return Arc::new(mock::MockTerminalHost::default());
    }
    Arc::new(SshTerminalHost { registry, sessions: Arc::default() })
}

/// The in-sandbox commands of one attach: the engine-exec argv that creates or resurrects the
/// zellij session, and the client command run over SSH. The session is never created over SSH,
/// because `sshd` drops the container's environment
/// (`docs/features/native-terminal/02_dev.md`, Resolved Decision 7).
fn session_commands(name: &str) -> (Vec<String>, String) {
    let create = ["zellij", "attach", "--create-background", name].map(String::from).to_vec();
    (create, format!("zellij attach {}", shell_quote(name)))
}

/// Quotes `word` for the remote shell `sshd` runs the client command through; plain words stay
/// bare.
fn shell_quote(word: &str) -> String {
    let plain = !word.is_empty() && word.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
    if plain {
        return word.to_string();
    }
    format!("'{}'", word.replace('\'', "'\\''"))
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

type SessionMap = Arc<Mutex<SessionTable>>;

struct SshTerminalHost {
    registry: Arc<ProviderRegistry>,
    sessions: SessionMap,
}

/// Everything the session task owns. The exec child is killed when the task drops it.
struct Session {
    handle: Handle<ssh::Client>,
    channel: SshChannel<Msg>,
    child: Child,
    /// Output that arrived before `sshd` accepted the command, forwarded first.
    early: Vec<Vec<u8>>,
}

impl Session {
    /// Ends a session that never ran: disconnects it and kills its exec child.
    async fn shut(self) {
        shut(self.handle, self.child).await;
    }
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
        let ((handle, mut channel), ()) = tokio::try_join!(handshake, self.create_session(&provider, &req.name))?;
        let (_, client) = session_commands(&req.name);
        channel.exec(true, client).await.map_err(|e| TerminalError::Handshake(e.to_string()))?;
        // `exec` only sends the request; `attach` must not report success before `sshd` accepts it.
        let early = ssh::await_reply(&mut channel, "the sandbox refused the terminal command").await?;
        Ok(Session { handle, channel, child, early })
    }

    async fn live_sender(&self, name: &str) -> Result<mpsc::Sender<SessionCommand>, TerminalError> {
        self.sessions.lock().await.sender(name).ok_or(TerminalError::NotAttached)
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
        // Registering before `open()` is what lets a detach or a newer attach cancel this one.
        let id = self.sessions.lock().await.begin(&req.name);
        let opened = tokio::time::timeout(ATTACH_TIMEOUT, self.open(&req))
            .await
            .map_err(|_| TerminalError::Handshake("timed out connecting to the sandbox".into()))
            .and_then(|r| r);
        let session = match opened {
            Ok(session) => session,
            Err(e) => {
                self.sessions.lock().await.abandon(&req.name, id);
                return Err(e);
            }
        };
        let (tx, rx) = mpsc::channel(COMMAND_QUEUE);
        let mut table = self.sessions.lock().await;
        if !table.promote(&req.name, id, tx) {
            drop(table);
            session.shut().await;
            return Err(TerminalError::Cancelled);
        }
        let task = tokio::spawn(run_session(session, rx, on_output, on_event, self.sessions.clone(), req.name.clone(), id));
        table.set_task(&req.name, id, task);
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
        self.sessions.lock().await.detach(name);
    }

    // Dropping the senders ends the tasks, which kill their exec children; the process may exit
    // before a detached task ever runs, and Windows does not kill children with their parent, so
    // the tasks are awaited here. On timeout, aborting drops each task's `kill_on_drop` child.
    async fn detach_all(&self) {
        let mut tasks = self.sessions.lock().await.drain();
        let wait = async {
            for task in tasks.iter_mut() {
                let _ = task.await;
            }
        };
        if tokio::time::timeout(EXIT_TIMEOUT, wait).await.is_err() {
            tasks.iter().for_each(|t| t.abort());
            let _ = tokio::time::timeout(Duration::from_millis(500), async {
                for task in tasks.iter_mut() {
                    let _ = task.await;
                }
            })
            .await;
        }
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
    let Session { handle, channel, child, early } = session;
    let (mut reader, writer) = channel.split();
    for data in early {
        if on_output.send(InvokeResponseBody::Raw(data)).is_err() {
            shut(handle, child).await;
            sessions.lock().await.finish(&name, id);
            return;
        }
    }
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
    shut(handle, child).await;
    sessions.lock().await.finish(&name, id);
}

/// Disconnects the transport and kills the exec child.
async fn shut(handle: Handle<ssh::Client>, mut child: Child) {
    let _ = handle.disconnect(Disconnect::ByApplication, "", "en").await;
    let _ = child.start_kill();
}

#[cfg(test)]
mod tests {
    use super::{session_already_running, session_commands, shell_quote};

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

    #[test]
    fn the_client_command_quotes_a_name_for_the_shell() {
        assert_eq!(shell_quote("demo-1.x_y"), "demo-1.x_y");
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("x;rm -rf /"), "'x;rm -rf /'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
        assert_eq!(shell_quote(""), "''");
        assert_eq!(session_commands("a$b").1, "zellij attach 'a$b'");
    }
}
