// Gating-spike probe for docs/features/herdr-multiplexer, adapted from the native-terminal ssh-probe.
// Carries SSH over `podman exec -i <container> mio-sshd` with an in-process russh client, requests a pty,
// runs ONE remote command and plays a script of steps, dumping every received byte to a file.
//
// Usage: herdr-probe <container> <identity> <outfile> <marker> <command> <script>
// Script steps are separated by ';':  w<ms> wait and collect | s<hex> send bytes | t<text> send text |
//   r<cols>x<rows> window-change | m<label> record a labelled offset into the byte dump | k drop (kill exec child, no SSH disconnect).
// Prints one JSON line: ms to first byte, ms to the first time the output contains <marker>, step offsets, byte count.
// Env: PODMAN_EXE, PODMAN_CONNECTION.
use std::pin::Pin;
use std::process::Stdio;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use russh::client::{self, Handler};
use russh::keys::{load_secret_key, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh::{ChannelMsg, Disconnect};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

struct Pipe {
    r: ChildStdout,
    w: ChildStdin,
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

struct H;
impl Handler for H {
    type Error = russh::Error;
    async fn check_server_key(&mut self, _k: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        // Spike only: the app verifies against the per-sandbox known-hosts file; that cost is a key compare.
        Ok(true)
    }
}

fn ms(t: Instant, t0: Instant) -> f64 {
    (t - t0).as_secs_f64() * 1000.0
}

fn podman_cmd() -> Command {
    let mut c = Command::new(std::env::var("PODMAN_EXE").expect("PODMAN_EXE"));
    c.args(["--connection", &std::env::var("PODMAN_CONNECTION").expect("PODMAN_CONNECTION")]);
    c
}

fn transport(name: &str) -> Command {
    let mut c = podman_cmd();
    c.args(["exec", "-i", name, "mio-sshd"]);
    c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true);
    c
}


fn contains(hay: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle)
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 7 {
        eprintln!("usage: herdr-probe <container> <identity> <outfile> <marker> <command> <script>");
        std::process::exit(2);
    }
    let (name, key_path, outfile, marker, command, script) = (&a[1], &a[2], &a[3], a[4].as_bytes(), &a[5], &a[6]);
    let key = load_secret_key(key_path, None)?;
    let t0 = Instant::now();
    let mut child: Child = transport(name).spawn()?;
    let pipe = Pipe { r: child.stdout.take().unwrap(), w: child.stdin.take().unwrap() };
    let cfg = Arc::new(client::Config { inactivity_timeout: Some(Duration::from_secs(120)), ..Default::default() });
    let mut sess = client::connect_stream(cfg, pipe, H).await?;
    if !sess.authenticate_publickey("root", PrivateKeyWithHashAlg::new(Arc::new(key), None)).await?.success() {
        return Err("authentication failed".into());
    }
    let mut ch = sess.channel_open_session().await?;
    ch.request_pty(true, "xterm-256color", 120, 40, 0, 0, &[]).await?;
    ch.exec(true, command.as_str()).await?;
    let t_cmd = ms(Instant::now(), t0);

    let mut buf: Vec<u8> = Vec::new();
    let mut first_byte: Option<f64> = None;
    let mut marker_at: Option<f64> = None;
    let mut labels: Vec<String> = Vec::new();
    let mut exit: Option<f64> = None;
    let mut dropped = false;

    // Collect for `dur`, or until the marker shows up when `dur` is 0 (bounded at 20 s).
    macro_rules! collect {
        ($dur:expr) => {{
            let dur: u64 = $dur;
            let until = Instant::now() + Duration::from_millis(if dur == 0 { 20_000 } else { dur });
            while Instant::now() < until {
                if let Ok(m) = tokio::time::timeout(Duration::from_millis(20), ch.wait()).await {
                    match m {
                        Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => {
                            if first_byte.is_none() {
                                first_byte = Some(ms(Instant::now(), t0));
                            }
                            buf.extend_from_slice(&data);
                            if marker_at.is_none() && contains(&buf, marker) {
                                marker_at = Some(ms(Instant::now(), t0));
                                if dur == 0 {
                                    break;
                                }
                            }
                        }
                        Some(ChannelMsg::ExitStatus { .. }) => exit = Some(ms(Instant::now(), t0)),
                        Some(_) => {}
                        None => break,
                    }
                }
            }
        }};
    }

    for step in script.split(';').filter(|s| !s.is_empty()) {
        let (op, arg) = step.split_at(1);
        match op {
            "w" => collect!(arg.parse().unwrap()),
            "s" => ch.data(&unhex(arg)[..]).await?,
            "t" => ch.data(arg.as_bytes()).await?,
            "r" => {
                let (c, r) = arg.split_once('x').unwrap();
                ch.window_change(c.parse().unwrap(), r.parse().unwrap(), 0, 0).await?;
            }
            "m" => labels.push(format!("\"{arg}\":{}", buf.len())),
            "k" => {
                // Simulates a dropped connection: the exec child dies with no SSH-level disconnect.
                let _ = child.kill().await;
                dropped = true;
                break;
            }
            _ => return Err("bad step".into()),
        }
    }
    if !dropped {
        let _ = ch.close().await;
        let _ = sess.disconnect(Disconnect::ByApplication, "", "en").await;
        let _ = child.kill().await;
    }
    std::fs::write(outfile, &buf)?;
    let f = |o: Option<f64>| o.map(|v| format!("{v:.1}")).unwrap_or("null".into());
    println!(
        "{{\"cmd_sent\":{t_cmd:.1},\"first_byte\":{},\"marker\":{},\"exit\":{},\"bytes\":{},\"offsets\":{{{}}}}}",
        f(first_byte),
        f(marker_at),
        f(exit),
        buf.len(),
        labels.join(",")
    );
    Ok(())
}
