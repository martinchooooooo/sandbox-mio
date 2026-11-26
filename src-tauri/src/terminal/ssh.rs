//! The SSH client half of the native terminal: a stream over an exec child's stdio and the russh
//! handler that pins the sandbox's host key.

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use russh::client::{self, Handle, Handler, Msg};
use russh::keys::{load_secret_key, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh::{Channel, ChannelMsg};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::process::{ChildStdin, ChildStdout};

use super::types::TerminalError;

/// The SSH user every sandbox identity is installed for.
const SSH_USER: &str = "root";
/// The port OpenSSH records the alias under; nothing listens on it, the exec carries the bytes.
const KNOWN_HOSTS_PORT: u16 = 22;
const TERM: &str = "xterm-256color";

/// The exec child's stdout and stdin joined into one duplex stream for russh.
pub struct Pipe {
    pub r: ChildStdout,
    pub w: ChildStdin,
}

impl AsyncRead for Pipe {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, b: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.r).poll_read(cx, b)
    }
}

impl AsyncWrite for Pipe {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, b: &[u8]) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.w).poll_write(cx, b)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.w).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.w).poll_shutdown(cx)
    }
}

#[derive(Debug)]
pub enum ClientError {
    Ssh(russh::Error),
    HostKeyChanged,
    KnownHosts(String),
}

impl From<russh::Error> for ClientError {
    fn from(e: russh::Error) -> Self {
        ClientError::Ssh(e)
    }
}

impl From<ClientError> for TerminalError {
    fn from(e: ClientError) -> Self {
        match e {
            ClientError::Ssh(e) => TerminalError::Handshake(e.to_string()),
            ClientError::HostKeyChanged => TerminalError::HostKeyChanged,
            ClientError::KnownHosts(m) => TerminalError::Handshake(m),
        }
    }
}

/// Checks the server key against the per-sandbox known-hosts file under the sandbox's alias, the
/// entry OpenSSH writes for it, and learns an absent key, like `StrictHostKeyChecking
/// accept-new` (`docs/features/native-terminal/02_dev.md`, Resolved Decision 10).
pub struct Client {
    host: String,
    known_hosts: String,
}

impl Handler for Client {
    type Error = ClientError;

    async fn check_server_key(&mut self, server_key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        let PublicKeyOrCertificate::PublicKey { key, .. } = server_key else {
            return Ok(false);
        };
        use russh::keys::known_hosts::{check_known_hosts_path, learn_known_hosts_path};
        match check_known_hosts_path(&self.host, KNOWN_HOSTS_PORT, key, &self.known_hosts) {
            Ok(true) => Ok(true),
            Ok(false) => {
                learn_known_hosts_path(&self.host, KNOWN_HOSTS_PORT, key, &self.known_hosts)
                    .map_err(|e| ClientError::KnownHosts(format!("recording the host key failed: {e}")))?;
                Ok(true)
            }
            Err(russh::keys::Error::KeyChanged { .. }) => Err(ClientError::HostKeyChanged),
            Err(e) => Err(ClientError::KnownHosts(format!("reading known hosts failed: {e}"))),
        }
    }
}

/// Runs the handshake over `pipe`, authenticates as `root`, and opens a session channel with a
/// pseudo-terminal of `cols` by `rows`.
pub async fn connect(
    pipe: Pipe,
    host: &str,
    identity_path: &str,
    known_hosts_path: &str,
    cols: u32,
    rows: u32,
) -> Result<(Handle<Client>, Channel<Msg>), TerminalError> {
    let key = load_secret_key(identity_path, None)
        .map_err(|e| TerminalError::Auth(format!("loading the sandbox identity failed: {e}")))?;
    // No inactivity timeout: an idle terminal is normal. Keepalives detect a dead transport.
    let config = Arc::new(client::Config {
        keepalive_interval: Some(Duration::from_secs(30)),
        keepalive_max: 3,
        ..Default::default()
    });
    let handler = Client { host: host.to_string(), known_hosts: known_hosts_path.to_string() };
    let mut session = client::connect_stream(config, pipe, handler).await?;
    let auth = session
        .authenticate_publickey(SSH_USER, PrivateKeyWithHashAlg::new(Arc::new(key), None))
        .await
        .map_err(|e| TerminalError::Auth(e.to_string()))?;
    if !auth.success() {
        return Err(TerminalError::Auth("public key rejected".into()));
    }
    let mut channel = session.channel_open_session().await.map_err(|e| TerminalError::Handshake(e.to_string()))?;
    channel
        .request_pty(true, TERM, cols, rows, 0, 0, &[])
        .await
        .map_err(|e| TerminalError::Handshake(e.to_string()))?;
    // Consumed here so the reply to the later exec request is the next one on the channel.
    await_reply(&mut channel, "the sandbox refused the pseudo-terminal").await?;
    Ok((session, channel))
}

/// Waits for the reply to the channel's last want-reply request. Output that arrives before the
/// reply is returned so the caller can forward it first. The caller bounds the wait.
pub async fn await_reply(channel: &mut Channel<Msg>, refused: &str) -> Result<Vec<Vec<u8>>, TerminalError> {
    let mut early = Vec::new();
    loop {
        match channel.wait().await {
            Some(ChannelMsg::Success) => return Ok(early),
            Some(ChannelMsg::Failure) => return Err(TerminalError::Handshake(refused.to_string())),
            Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => early.push(data.to_vec()),
            Some(ChannelMsg::Close) | None => {
                return Err(TerminalError::Handshake("the connection closed before the terminal started".into()))
            }
            Some(_) => {}
        }
    }
}
