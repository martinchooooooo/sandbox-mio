//! Host-side readiness probe for a sidecar's published SOCKS5 listener. The engine reports a
//! sidecar running about 3 s before mitmdump listens
//! (`docs/features/microsandbox-network-policy/spike/04_msb-mitm-sidecar.md`, Item 6), so the
//! agent must not be pointed at it until a full SOCKS5 greeting succeeds.

use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use super::types::{ProviderError, SidecarReadiness};

/// Startup budget the spike measured headroom for.
pub const DEFAULT_BUDGET_MS: u64 = 5000;

/// One SOCKS5 greeting (RFC 1928) plus username/password sub-negotiation (RFC 1929).
async fn greet(port: u16, auth: Option<&(String, String)>) -> std::io::Result<bool> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await?;
    let method = if auth.is_some() { 0x02 } else { 0x00 };
    stream.write_all(&[0x05, 0x01, method]).await?;
    let mut reply = [0u8; 2];
    stream.read_exact(&mut reply).await?;
    if reply != [0x05, method] {
        return Ok(false);
    }
    let Some((user, pass)) = auth else {
        return Ok(true);
    };
    let mut msg = vec![0x01, user.len() as u8];
    msg.extend_from_slice(user.as_bytes());
    msg.push(pass.len() as u8);
    msg.extend_from_slice(pass.as_bytes());
    stream.write_all(&msg).await?;
    stream.read_exact(&mut reply).await?;
    Ok(reply == [0x01, 0x00])
}

/// Probes until a SOCKS5 greeting succeeds or the budget runs out. Returns at once under
/// `MIO_MOCK_BACKEND=1`, where no listener exists.
pub async fn wait_sidecar_ready(readiness: &SidecarReadiness) -> Result<(), ProviderError> {
    if crate::backends::mock_backend_enabled() {
        return Ok(());
    }
    let deadline = Instant::now() + Duration::from_millis(readiness.budget_ms);
    loop {
        let attempt = tokio::time::timeout(
            Duration::from_millis(1000),
            greet(readiness.host_port, readiness.socks5_auth.as_ref()),
        )
        .await;
        let last = match attempt {
            Ok(Ok(true)) => return Ok(()),
            Ok(Ok(false)) => "the listener refused the SOCKS5 greeting".to_string(),
            Ok(Err(e)) => e.to_string(),
            Err(_) => "the SOCKS5 greeting timed out".to_string(),
        };
        if Instant::now() >= deadline {
            return Err(ProviderError::CommandFailed(format!(
                "proxy on 127.0.0.1:{} is not answering: {last}",
                readiness.host_port
            )));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// A single non-waiting probe, for status reporting.
pub async fn sidecar_healthy(readiness: &SidecarReadiness) -> bool {
    if crate::backends::mock_backend_enabled() {
        return true;
    }
    matches!(
        tokio::time::timeout(
            Duration::from_millis(1000),
            greet(readiness.host_port, readiness.socks5_auth.as_ref())
        )
        .await,
        Ok(Ok(true))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal in-process SOCKS5 server accepting one fixed credential pair.
    async fn serve(listener: tokio::net::TcpListener) {
        while let Ok((mut s, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut head = [0u8; 3];
                if s.read_exact(&mut head).await.is_err() {
                    return;
                }
                let _ = s.write_all(&[0x05, 0x02]).await;
                let mut buf = [0u8; 64];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let ok = &buf[..n] == b"\x01\x03mio\x02pw";
                let _ = s.write_all(&[0x01, if ok { 0x00 } else { 0x01 }]).await;
            });
        }
    }

    #[tokio::test]
    async fn accepts_the_right_password_and_rejects_a_wrong_one() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(serve(listener));
        let good = SidecarReadiness {
            host_port: port,
            socks5_auth: Some(("mio".to_string(), "pw".to_string())),
            budget_ms: 1000,
        };
        wait_sidecar_ready(&good).await.expect("ready");
        let bad = SidecarReadiness { socks5_auth: Some(("mio".into(), "no".into())), budget_ms: 300, ..good };
        assert!(wait_sidecar_ready(&bad).await.is_err());
    }
}
