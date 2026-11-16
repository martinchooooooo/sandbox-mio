//! `MockTerminalHost`: an echoing terminal under `MIO_MOCK_BACKEND=1`, so the `light` tier exercises
//! the terminal pane without an engine.

use std::collections::HashMap;

use tauri::ipc::{Channel, InvokeResponseBody};
use tokio::sync::Mutex;

use super::types::{AttachRequest, TerminalError, TerminalEvent, TerminalHost};

struct Session {
    output: Channel<InvokeResponseBody>,
    events: Channel<TerminalEvent>,
}

impl Session {
    fn write(&self, bytes: &[u8]) {
        let _ = self.output.send(InvokeResponseBody::Raw(bytes.to_vec()));
    }

    fn end(self) {
        let _ = self.events.send(TerminalEvent::Exited { reason: "detached".into(), exit_status: None });
    }
}

#[derive(Default)]
pub struct MockTerminalHost {
    sessions: Mutex<HashMap<String, Session>>,
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
impl TerminalHost for MockTerminalHost {
    async fn attach(
        &self,
        req: AttachRequest,
        on_output: Channel<InvokeResponseBody>,
        on_event: Channel<TerminalEvent>,
    ) -> Result<(), TerminalError> {
        let session = Session { output: on_output, events: on_event };
        session.write(format!("mio mock terminal {}\r\n$ ", req.name).as_bytes());
        if let Some(old) = self.sessions.lock().await.insert(req.name, session) {
            old.end();
        }
        Ok(())
    }

    async fn input(&self, name: &str, bytes: Vec<u8>) -> Result<(), TerminalError> {
        let sessions = self.sessions.lock().await;
        sessions.get(name).ok_or(TerminalError::NotAttached)?.write(&echo(&bytes));
        Ok(())
    }

    async fn resize(&self, name: &str, cols: u32, rows: u32) -> Result<(), TerminalError> {
        let sessions = self.sessions.lock().await;
        sessions.get(name).ok_or(TerminalError::NotAttached)?.write(format!("[resize {cols}x{rows}]").as_bytes());
        Ok(())
    }

    async fn detach(&self, name: &str) {
        if let Some(session) = self.sessions.lock().await.remove(name) {
            session.end();
        }
    }

    async fn detach_all(&self) {
        for (_, session) in self.sessions.lock().await.drain() {
            session.end();
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_carriage_return_echoes_as_a_new_line() {
        assert_eq!(super::echo(b"ls\r"), b"ls\r\n");
    }
}
