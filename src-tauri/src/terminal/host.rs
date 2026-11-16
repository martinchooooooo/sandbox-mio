//! The session lifecycle both terminal hosts share: the attach race, the per-session command queue
//! and task, and the exit cleanup. Only the transport step differs between hosts, behind
//! [`Transport`] and [`Link`] (`docs/features/native-terminal/02_dev.md`, Implemented: Review fixes).

use std::sync::Arc;
use std::time::Duration;

use tauri::ipc::{Channel, InvokeResponseBody};
use tokio::sync::{mpsc, Mutex};

use super::sessions::SessionTable;
use super::types::{AttachRequest, TerminalError, TerminalEvent, TerminalHost};

/// Bounds the transport's `open`; a remote Podman exec is the slowest step in it.
const ATTACH_TIMEOUT: Duration = Duration::from_secs(60);
/// Total time `detach_all` waits for session tasks to end on app exit.
const EXIT_TIMEOUT: Duration = Duration::from_secs(3);
/// Input and resize commands queued per session before a sender waits.
const COMMAND_QUEUE: usize = 256;

pub(super) enum SessionCommand {
    Input(Vec<u8>),
    Resize(u32, u32),
}

/// Why a session's pump stopped.
pub(super) struct Ended {
    pub(super) reason: &'static str,
    pub(super) exit_status: Option<u32>,
}

/// The step that differs between hosts: opening a session.
#[async_trait::async_trait]
pub(super) trait Transport: Send + Sync + 'static {
    type Link: Link;

    /// Opens a session for `req`. Returns once it is ready to pump; the lifecycle bounds the call
    /// with the attach timeout.
    async fn open(&self, req: &AttachRequest) -> Result<Self::Link, TerminalError>;
}

/// One opened session. The lifecycle owns it from `open` until the session task ends.
#[async_trait::async_trait]
pub(super) trait Link: Send + 'static {
    /// Pumps output to `output` and `commands` to the session until either side ends.
    async fn pump(&mut self, commands: &mut mpsc::Receiver<SessionCommand>, output: &Channel<InvokeResponseBody>) -> Ended;
    /// Releases the transport, whether or not the session ever ran.
    async fn shut(self);
}

type SessionMap = Arc<Mutex<SessionTable>>;

/// A [`TerminalHost`] whose lifecycle is fixed and whose transport is `T`.
pub(super) struct LifecycleHost<T: Transport> {
    transport: T,
    sessions: SessionMap,
}

impl<T: Transport> LifecycleHost<T> {
    pub(super) fn new(transport: T) -> Self {
        Self { transport, sessions: Arc::default() }
    }

    async fn live_sender(&self, name: &str) -> Result<mpsc::Sender<SessionCommand>, TerminalError> {
        self.sessions.lock().await.sender(name).ok_or(TerminalError::NotAttached)
    }
}

#[async_trait::async_trait]
impl<T: Transport> TerminalHost for LifecycleHost<T> {
    async fn attach(
        &self,
        req: AttachRequest,
        on_output: Channel<InvokeResponseBody>,
        on_event: Channel<TerminalEvent>,
    ) -> Result<(), TerminalError> {
        // Registering before `open()` is what lets a detach or a newer attach cancel this one.
        let id = self.sessions.lock().await.begin(&req.name);
        let opened = tokio::time::timeout(ATTACH_TIMEOUT, self.transport.open(&req))
            .await
            .map_err(|_| TerminalError::Handshake("timed out connecting to the sandbox".into()))
            .and_then(|r| r);
        let link = match opened {
            Ok(link) => link,
            Err(e) => {
                self.sessions.lock().await.abandon(&req.name, id);
                return Err(e);
            }
        };
        let (tx, rx) = mpsc::channel(COMMAND_QUEUE);
        let mut table = self.sessions.lock().await;
        if !table.promote(&req.name, id, tx) {
            drop(table);
            link.shut().await;
            return Err(TerminalError::Cancelled);
        }
        let task = tokio::spawn(run_session(link, rx, on_output, on_event, self.sessions.clone(), req.name.clone(), id));
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

/// Pumps the link until it ends, then reports the exit and drops the map entry unless a newer
/// session has replaced it.
async fn run_session<L: Link>(
    mut link: L,
    mut commands: mpsc::Receiver<SessionCommand>,
    on_output: Channel<InvokeResponseBody>,
    on_event: Channel<TerminalEvent>,
    sessions: SessionMap,
    name: String,
    id: u64,
) {
    let Ended { reason, exit_status } = link.pump(&mut commands, &on_output).await;
    tracing::info!(target: "terminal", sandbox = %name, reason, ?exit_status, "terminal session ended");
    let _ = on_event.send(TerminalEvent::Exited { reason: reason.to_string(), exit_status });
    link.shut().await;
    sessions.lock().await.finish(&name, id);
}
