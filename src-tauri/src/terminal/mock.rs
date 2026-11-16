//! `MockTransport`: an echoing, scriptable terminal transport under `MIO_MOCK_BACKEND=1`, so the
//! `light` tier exercises the real session lifecycle (`host.rs`) without an engine.

use std::sync::Arc;

use tauri::ipc::{Channel, InvokeResponseBody};
use tokio::sync::mpsc;

use super::host::{Ended, Link, SessionCommand, Transport};
use super::types::{AttachRequest, TerminalError};
use crate::mockctl::types::MockDirector;

/// The call key of an open, scripted and journalled through the mock director. Must equal the
/// `terminal.open` entry in `KNOWN_CALLS` in `src-tauri/src/mockctl/mod.rs`.
const OPEN_KEY: &str = "terminal.open";

pub(super) struct MockTransport {
    director: Arc<MockDirector>,
}

impl MockTransport {
    pub(super) fn new(director: Arc<MockDirector>) -> Self {
        Self { director }
    }
}

/// An opened mock session: a banner, then echoed input and resize reports.
pub(super) struct MockLink {
    name: String,
}

/// Echoes input as a line-mode terminal would: a carriage return also moves to a new line.
fn echo(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    for &b in bytes {
        out.push(b);
        if b == b'\r' {
            out.push(b'\n');
        }
    }
    out
}

#[async_trait::async_trait]
impl Transport for MockTransport {
    type Link = MockLink;

    /// A scripted failure fails the open as a refused session command; a scripted delay holds it.
    async fn open(&self, req: &AttachRequest) -> Result<MockLink, TerminalError> {
        self.director.record(OPEN_KEY, &[req.name.as_str()], &[]);
        if let Some(scripted) = self.director.script(OPEN_KEY).await {
            if scripted.exit_code != 0 {
                return Err(TerminalError::Handshake(scripted.stderr));
            }
        }
        Ok(MockLink { name: req.name.clone() })
    }
}

#[async_trait::async_trait]
impl Link for MockLink {
    async fn pump(&mut self, commands: &mut mpsc::Receiver<SessionCommand>, output: &Channel<InvokeResponseBody>) -> Ended {
        let write = |bytes: Vec<u8>| output.send(InvokeResponseBody::Raw(bytes)).is_ok();
        if !write(format!("mio mock terminal {}\r\n$ ", self.name).into_bytes()) {
            return Ended { reason: "window closed", exit_status: None };
        }
        loop {
            let sent = match commands.recv().await {
                Some(SessionCommand::Input(bytes)) => write(echo(&bytes)),
                Some(SessionCommand::Resize(cols, rows)) => write(format!("[resize {cols}x{rows}]").into_bytes()),
                None => return Ended { reason: "detached", exit_status: None },
            };
            if !sent {
                return Ended { reason: "window closed", exit_status: None };
            }
        }
    }

    async fn shut(self) {}
}
