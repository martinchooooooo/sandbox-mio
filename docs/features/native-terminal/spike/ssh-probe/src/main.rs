// Gating-spike probe for docs/features/native-terminal: an in-process russh client that carries SSH over the
// stdio of `podman exec -i <container> mio-sshd` (or `msb exec --stream <name> -- mio-sshd`), requests a pty,
// and runs one of several remote commands, printing one JSON object of millisecond timestamps per iteration.
//
// Usage: ssh-probe <engine:podman|msb> <container> <mode> <iterations> <identity-file>
// Modes: true | shell | zellij | zellij-new | stty
// Env: PODMAN_EXE, PODMAN_CONNECTION (podman); MSB_EXE (msb).
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

fn transport(engine: &str, name: &str) -> Command {
    let mut c;
    if engine == "msb" {
        c = Command::new(std::env::var("MSB_EXE").expect("MSB_EXE"));
        c.args(["exec", "--stream", name, "--", "mio-sshd"]);
    } else {
        c = podman_cmd();
        c.args(["exec", "-i", name, "mio-sshd"]);
    }
    c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true);
    c
}

/// Largest (row, col) among CSI `row;colH` cursor-position sequences in `buf`.
fn max_cursor(buf: &[u8]) -> (u32, u32) {
    let (mut mr, mut mc) = (0, 0);
    let mut i = 0;
    while i + 2 < buf.len() {
        if buf[i] == 0x1b && buf[i + 1] == b'[' {
            let mut j = i + 2;
            while j < buf.len() && (buf[j].is_ascii_digit() || buf[j] == b';') {
                j += 1;
            }
            if j < buf.len() && (buf[j] == b'H' || buf[j] == b'f') {
                let body = std::str::from_utf8(&buf[i + 2..j]).unwrap_or("");
                let mut it = body.split(';');
                let r: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                let c: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                mr = mr.max(r);
                mc = mc.max(c);
            }
            i = j;
        }
        i += 1;
    }
    (mr, mc)
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

async fn size(ch: &mut russh::Channel<client::Msg>, marker: &str) -> String {
    ch.data(format!("stty size; echo {marker}\n").as_bytes()).await.ok();
    let mut s = Vec::new();
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        if let Ok(Some(ChannelMsg::Data { data })) = tokio::time::timeout(Duration::from_millis(100), ch.wait()).await {
            s.extend_from_slice(&data);
            let t = String::from_utf8_lossy(&s).to_string();
            // The marker appears once in the echoed command line and once as output.
            if t.matches(marker).count() >= 2 {
                return t;
            }
        }
    }
    String::from_utf8_lossy(&s).to_string()
}

fn grab_size(t: &str) -> String {
    t.split(|c: char| c.is_control())
        .map(|l| l.trim())
        .find(|l| l.split(' ').count() == 2 && l.chars().all(|c| c.is_ascii_digit() || c == ' '))
        .unwrap_or("?")
        .to_string()
}

async fn one(engine: &str, name: &str, mode: &str, key_path: &str) -> Result<String, Box<dyn std::error::Error>> {
    if mode == "zellij-new" {
        // Outside the timed region: drop every zellij session so attach has to create one.
        let mut c = podman_cmd();
        c.args(["exec", name, "zellij", "kill-all-sessions", "-y"]);
        let _ = c.output().await;
        let mut c = podman_cmd();
        c.args(["exec", name, "zellij", "delete-all-sessions", "-y", "-f"]);
        let _ = c.output().await;
    }
    let key = load_secret_key(key_path, None)?;

    let t0 = Instant::now();
    let mut child: Child = transport(engine, name).spawn()?;
    let t_spawn = Instant::now();
    let pipe = Pipe { r: child.stdout.take().unwrap(), w: child.stdin.take().unwrap() };
    let cfg = Arc::new(client::Config { inactivity_timeout: Some(Duration::from_secs(30)), ..Default::default() });
    let mut sess = client::connect_stream(cfg, pipe, H).await?;
    let t_kex = Instant::now();
    let auth = sess.authenticate_publickey("root", PrivateKeyWithHashAlg::new(Arc::new(key), None)).await?;
    if !auth.success() {
        return Err("authentication failed".into());
    }
    let t_auth = Instant::now();
    let mut ch = sess.channel_open_session().await?;
    let t_chan = Instant::now();
    ch.request_pty(true, "xterm-256color", 120, 40, 0, 0, &[]).await?;
    let t_pty = Instant::now();

    let mut extra = String::new();
    match mode {
        "true" => ch.exec(true, "true").await?,
        "shell" | "stty" => ch.request_shell(true).await?,
        "zellij" | "zellij-new" => ch.exec(true, format!("zellij attach --create {name}")).await?,
        _ => return Err("unknown mode".into()),
    }
    let t_exec = Instant::now();

    let mut first_byte: Option<f64> = None;
    let mut frame: Option<f64> = None; // first time the zellij tab bar and status bar are both in the output
    let mut prompt: Option<f64> = None; // shell prompt ending in "# "
    let mut exit: Option<f64> = None;
    let mut buf: Vec<u8> = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(20);
    let want_frame = mode.starts_with("zellij");

    loop {
        if Instant::now() > deadline {
            extra.push_str(",\"timeout\":true");
            break;
        }
        let msg = match tokio::time::timeout(Duration::from_millis(500), ch.wait()).await {
            Ok(m) => m,
            Err(_) => continue,
        };
        match msg {
            Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => {
                if first_byte.is_none() {
                    first_byte = Some(ms(Instant::now(), t0));
                }
                buf.extend_from_slice(&data);
                if want_frame && frame.is_none() && contains(&buf, b"Tab #1") && contains(&buf, b"Ctrl") {
                    frame = Some(ms(Instant::now(), t0));
                }
                if (mode == "shell" || mode == "stty") && prompt.is_none() && buf.ends_with(b"# ") {
                    prompt = Some(ms(Instant::now(), t0));
                }
                if (want_frame && frame.is_some()) || ((mode == "shell" || mode == "stty") && prompt.is_some()) {
                    break;
                }
            }
            Some(ChannelMsg::ExitStatus { .. }) => {
                exit = Some(ms(Instant::now(), t0));
                if mode == "true" {
                    break;
                }
            }
            Some(_) => {}
            None => break,
        }
    }

    // Resize check on zellij: let the first frame settle, record its extent, send window-change, record the redraw extent.
    if want_frame && frame.is_some() {
        tokio::time::sleep(Duration::from_millis(500)).await;
        while let Ok(Some(m)) = tokio::time::timeout(Duration::from_millis(50), ch.wait()).await {
            if let ChannelMsg::Data { data } = m {
                buf.extend_from_slice(&data);
            }
        }
        let (r0, c0) = max_cursor(&buf);
        let tw = Instant::now();
        ch.window_change(160, 50, 0, 0).await?;
        let mut first_after: Option<f64> = None;
        let mut after: Vec<u8> = Vec::new();
        let mut last = Instant::now();
        while last.elapsed() < Duration::from_millis(400) && tw.elapsed() < Duration::from_secs(5) {
            if let Ok(Some(ChannelMsg::Data { data })) = tokio::time::timeout(Duration::from_millis(50), ch.wait()).await {
                if first_after.is_none() {
                    first_after = Some(ms(Instant::now(), tw));
                }
                after.extend_from_slice(&data);
                last = Instant::now();
            }
        }
        let (r1, c1) = max_cursor(&after);
        extra.push_str(&format!(
            ",\"frame1_max_row\":{r0},\"frame1_max_col\":{c0},\"resize_first_byte_ms\":{},\"resize_bytes\":{},\"frame2_max_row\":{r1},\"frame2_max_col\":{c1}",
            first_after.map(|v| format!("{v:.1}")).unwrap_or("null".into()),
            after.len()
        ));
    }
    if mode == "stty" {
        // Plain shell: ask the pty its size before and after window-change.
        let before = size(&mut ch, "MARK1").await;
        ch.window_change(160, 50, 0, 0).await?;
        tokio::time::sleep(Duration::from_millis(100)).await;
        let after = size(&mut ch, "MARK2").await;
        extra.push_str(&format!(",\"stty_before\":\"{}\",\"stty_after\":\"{}\"", grab_size(&before), grab_size(&after)));
    }

    let t_done = Instant::now();
    let _ = ch.close().await;
    let _ = sess.disconnect(Disconnect::ByApplication, "", "en").await;
    let _ = child.kill().await;
    let f = |o: Option<f64>| o.map(|v| format!("{v:.1}")).unwrap_or("null".into());
    Ok(format!(
        "{{\"mode\":\"{mode}\",\"spawn\":{:.1},\"kex\":{:.1},\"auth\":{:.1},\"chan\":{:.1},\"pty\":{:.1},\"exec_sent\":{:.1},\"first_byte\":{},\"frame\":{},\"prompt\":{},\"exit\":{},\"total\":{:.1}{extra}}}",
        ms(t_spawn, t0),
        ms(t_kex, t0),
        ms(t_auth, t0),
        ms(t_chan, t0),
        ms(t_pty, t0),
        ms(t_exec, t0),
        f(first_byte),
        f(frame),
        f(prompt),
        f(exit),
        ms(t_done, t0)
    ))
}

#[tokio::main]
async fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 6 {
        eprintln!("usage: ssh-probe <podman|msb> <container> <mode> <iterations> <identity>");
        std::process::exit(2);
    }
    let n: usize = a[4].parse().unwrap();
    for _ in 0..n {
        match one(&a[1], &a[2], &a[3], &a[5]).await {
            Ok(j) => println!("{j}"),
            Err(e) => println!("{{\"mode\":\"{}\",\"error\":\"{}\"}}", a[3], e.to_string().replace('"', "'")),
        }
    }
}
