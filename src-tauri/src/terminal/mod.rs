//! The terminal host subsystem: the native terminal's host side, which carries an SSH session over
//! the engine's streamed exec and attaches it to the sandbox's Herdr session
//! (`docs/features/native-terminal/02_dev.md`, `docs/features/herdr-multiplexer/02_dev.md`).

mod host;
mod mock;
mod sessions;
mod ssh;
pub mod types;

use std::process::Stdio;
use std::sync::Arc;

use sha2::{Digest, Sha256};
use russh::client::{Handle, Msg};
use russh::{Channel as SshChannel, ChannelMsg, Disconnect};
use tauri::ipc::{Channel, InvokeResponseBody};
use tokio::process::Child;
use tokio::sync::mpsc;

use mio_mockctl::types::MockDirector;
use mio_provider::types::{ExecOpts, MachineProvider};
use mio_provider::ProviderRegistry;
use host::{Ended, LifecycleHost, Link, SessionCommand, Transport};
use types::{AttachRequest, TerminalError, TerminalHost};

/// The working directory of every pane a new Herdr session opens. Must equal `WORKSPACE_MOUNT`
/// in `src-tauri/crates/provider/src/lib.rs`, which is private there.
const WORKDIR: &str = "/root/work";

/// Constructs the terminal host: the scriptable echo transport under `MIO_MOCK_BACKEND=1`, because
/// the session's exec child bypasses the scripted `ProcessRunner` and would reach a real engine.
/// Both transports run the same session lifecycle.
pub fn default_terminal_host(registry: Arc<ProviderRegistry>, director: Arc<MockDirector>) -> Arc<dyn TerminalHost> {
    if mio_domain::mock_backend_enabled() {
        return Arc::new(LifecycleHost::new(mock::MockTransport::new(director)));
    }
    Arc::new(LifecycleHost::new(SshTransport { registry }))
}

/// Longest Herdr session name that keeps its socket path under the `sun_path` limit.
const SESSION_NAME_MAX: usize = 61;

/// Characters of a long name kept before the hash suffix (`52 + 1 + 8 = SESSION_NAME_MAX`).
const SESSION_NAME_PREFIX: usize = 52;

/// The Herdr session name for a sandbox: the name itself when its socket path fits, otherwise a
/// truncated prefix plus a hash so two long names sharing a prefix stay apart
/// (`docs/features/herdr-multiplexer/02_dev.md`, Resolved Decision 4).
fn session_name(sandbox: &str) -> String {
    if sandbox.chars().count() <= SESSION_NAME_MAX {
        return sandbox.to_string();
    }
    let prefix: String = sandbox.chars().take(SESSION_NAME_PREFIX).collect();
    let digest = Sha256::digest(sandbox.as_bytes());
    let hex: String = digest.iter().take(4).map(|b| format!("{b:02x}")).collect();
    format!("{prefix}-{hex}")
}

/// Exit code of the start script when `herdr` is not on `PATH`.
const EXIT_MULTIPLEXER_MISSING: i32 = 127;

/// Starts the session's Herdr server unless it already runs, and returns only once it reports
/// running. The check and the poll are foreground on purpose: an SSH client arriving first would
/// start its own server with the stripped `sshd` environment
/// (`docs/features/herdr-multiplexer/02_dev.md`, Resolved Decision 3).
const START_SCRIPT: &str = r#"command -v herdr >/dev/null 2>&1 || exit 127
herdr --session "$1" status server 2>/dev/null | grep -q 'status: running' && exit 0
setsid herdr --session "$1" server </dev/null >/dev/null 2>&1 &
i=0
while [ "$i" -lt 50 ]; do
  herdr --session "$1" status server 2>/dev/null | grep -q 'status: running' && exit 0
  sleep 0.1
  i=$((i+1))
done
exit 1"#;

/// The engine-exec argv that creates or resurrects the Herdr server for `session`. The server is
/// never started over SSH, because `sshd` drops the container's environment
/// (`docs/features/native-terminal/02_dev.md`, Resolved Decision 7).
fn create_argv(session: &str) -> Vec<String> {
    ["sh", "-c", START_SCRIPT, "sh", session].map(String::from).to_vec()
}

/// The client command run over SSH, attaching to the session `create_argv` started.
fn client_command(session: &str) -> String {
    format!("herdr --session {}", shell_quote(session))
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

struct SshTransport {
    registry: Arc<ProviderRegistry>,
}

/// Everything an opened SSH session owns. The exec child is killed when it drops.
struct Session {
    handle: Handle<ssh::Client>,
    /// Taken by `pump`, which splits it.
    channel: Option<SshChannel<Msg>>,
    child: Child,
    /// Output that arrived before `sshd` accepted the command, forwarded first.
    early: Vec<Vec<u8>>,
}

impl SshTransport {
    async fn create_session(&self, provider: &Arc<dyn MachineProvider>, name: &str) -> Result<(), TerminalError> {
        let argv = create_argv(&session_name(name));
        let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
        let opts = ExecOpts { workdir: Some(WORKDIR.to_string()), ..Default::default() };
        let out = provider.exec(name, &argv, opts).await?;
        if !out.status.success() {
            if out.status.code() == Some(EXIT_MULTIPLEXER_MISSING) {
                return Err(TerminalError::MultiplexerMissing);
            }
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            return Err(TerminalError::SessionCreate(stderr));
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl Transport for SshTransport {
    type Link = Session;

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
            &req.alias,
            &req.identity_path,
            &req.known_hosts_path,
            req.cols,
            req.rows,
        );
        let ((handle, mut channel), ()) = tokio::try_join!(handshake, self.create_session(&provider, &req.name))?;
        let client = client_command(&session_name(&req.name));
        channel.exec(true, client).await.map_err(|e| TerminalError::Handshake(e.to_string()))?;
        // `exec` only sends the request; `attach` must not report success before `sshd` accepts it.
        let early = ssh::await_reply(&mut channel, "the sandbox refused the terminal command").await?;
        Ok(Session { handle, channel: Some(channel), child, early })
    }
}

#[async_trait::async_trait]
impl Link for Session {
    async fn pump(&mut self, commands: &mut mpsc::Receiver<SessionCommand>, output: &Channel<InvokeResponseBody>) -> Ended {
        let Some(channel) = self.channel.take() else {
            return Ended { reason: "connection closed", exit_status: None };
        };
        let (mut reader, writer) = channel.split();
        for data in std::mem::take(&mut self.early) {
            if output.send(InvokeResponseBody::Raw(data)).is_err() {
                return Ended { reason: "window closed", exit_status: None };
            }
        }
        let (reason, exit_status) = loop {
            tokio::select! {
                msg = reader.wait() => match msg {
                    Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => {
                        if output.send(InvokeResponseBody::Raw(data.to_vec())).is_err() {
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
        Ended { reason, exit_status }
    }

    /// Disconnects the transport and kills the exec child.
    async fn shut(self) {
        let _ = self.handle.disconnect(Disconnect::ByApplication, "", "en").await;
        let mut child = self.child;
        let _ = child.start_kill();
    }
}

#[cfg(test)]
mod tests {
    use super::{client_command, create_argv, session_name, shell_quote, START_SCRIPT};

    #[test]
    fn the_server_is_started_by_exec_and_only_attached_over_ssh() {
        assert_eq!(create_argv("demo"), ["sh", "-c", START_SCRIPT, "sh", "demo"]);
        assert_eq!(client_command("demo"), "herdr --session demo");
    }

    #[test]
    fn the_client_command_quotes_a_name_for_the_shell() {
        assert_eq!(shell_quote("demo-1.x_y"), "demo-1.x_y");
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("x;rm -rf /"), "'x;rm -rf /'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
        assert_eq!(shell_quote(""), "''");
        assert_eq!(client_command("a$b"), "herdr --session 'a$b'");
    }

    #[test]
    fn a_session_name_passes_through_up_to_the_socket_limit() {
        assert_eq!(session_name("demo"), "demo");
        let at_limit = "a".repeat(61);
        assert_eq!(session_name(&at_limit), at_limit);
    }

    #[test]
    fn a_longer_name_is_truncated_with_a_hash_suffix() {
        let name = "a".repeat(62);
        let session = session_name(&name);
        assert_eq!(session.len(), 61);
        assert!(session.starts_with(&"a".repeat(52)));
        assert_eq!(session.as_bytes()[52], b'-');
        assert_eq!(session, session_name(&name));
        let other = format!("{}b", "a".repeat(61));
        assert_ne!(session_name(&other), session);
        assert_eq!(session_name(&other).len(), 61);
    }
}
