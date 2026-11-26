//! Poll loop over the proxy sidecar's NDJSON flow log
//! (`docs/features/proxy-traffic-viewer/02_dev.md`). One detached task per watched sandbox parses
//! only newly appended lines and pushes them to the frontend as `proxy://flows-changed`. Each tick
//! first ingests the iron-proxy audit stream into the file ([`super::audit`]).

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Emitter};

use super::store::ProxyStore;
use super::types::{ProxyEngine, SidecarSite};

const EVENT_NAME: &str = "proxy://flows-changed";

/// Cap on retained flows. Must equal `FLOW_LOG_MAX_LINES` in `src/lib/proxy/traffic.svelte.ts`.
pub const FLOW_LOG_MAX_LINES: usize = 500;

/// Matches `PORTS_FILE_POLL_INTERVAL` (`src-tauri/src/network/mod.rs`): a bind-mounted file read is
/// cheap enough to repeat this often, and frequent enough that the panel doesn't feel laggy
/// (Resolved Decision 6).
const FLOW_LOG_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// One finished flow translated from iron-proxy's audit stream. Metadata only — never headers,
/// bodies, or an injected secret value (`docs/features/proxy-traffic-viewer/01_ideas.md`, Capture Scope).
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlowEntry {
    pub id: u64,
    pub method: String,
    pub host: String,
    pub path: String,
    pub status: Option<u16>,
    pub ts: f64,
    /// The `error()`-hook entries carry none of the fields below, so each one defaults.
    #[serde(default)]
    pub duration_ms: Option<f64>,
    #[serde(default)]
    pub req_bytes: Option<u64>,
    #[serde(default)]
    pub resp_bytes: Option<u64>,
    #[serde(default)]
    pub secret_injected: bool,
    #[serde(default)]
    pub error: Option<String>,
    /// The iron-proxy transform that refused the request.
    #[serde(default)]
    pub rejected_by: Option<String>,
    /// An `open`-mode request an allowlist policy would have refused.
    #[serde(default)]
    pub would_block: bool,
}

/// One poll task per watched sandbox, counted by the panels watching it, so a repeated
/// `watch_proxy_flows` call shares the task and the last `unwatch_proxy_flows` stops it.
#[derive(Default)]
pub struct FlowWatchRegistry(Mutex<HashMap<String, Watch>>);

struct Watch {
    handle: tauri::async_runtime::JoinHandle<()>,
    watchers: usize,
}

/// Counts one more watcher of `sandbox`, starting its poll task unless a live one is registered;
/// returns whether a new task was spawned. Check and insert happen under one lock — two rapid calls
/// would otherwise both spawn and double every row.
///
/// Spawns through `tauri::async_runtime`, not `tokio::spawn`: a caller on the IPC thread has no
/// Tokio runtime entered, and `tokio::spawn`'s panic there cannot unwind across the webview's FFI
/// boundary — it aborts the process.
pub fn start_watch(
    app: &AppHandle,
    store: &ProxyStore,
    registry: &FlowWatchRegistry,
    site: &SidecarSite,
    sandbox: &str,
) -> bool {
    let mut tasks = registry.0.lock().unwrap();
    let watchers = match tasks.get_mut(sandbox) {
        Some(watch) if !watch.handle.inner().is_finished() => {
            watch.watchers += 1;
            return false;
        }
        // A task that ended (its log disappeared) keeps the count of panels still mounted.
        Some(watch) => watch.watchers + 1,
        None => 1,
    };
    let handle = tauri::async_runtime::spawn(watch_flows(
        app.clone(),
        store.clone(),
        site.clone(),
        sandbox.to_string(),
    ));
    tasks.insert(sandbox.to_string(), Watch { handle, watchers });
    true
}

/// Points a running watch of `sandbox` at `site`, keeping its watcher count, so deploy and undeploy
/// move the traffic panel to the active sidecar. Does nothing when no panel watches it; the next
/// `start_watch` resolves the site afresh.
pub fn retarget(
    app: &AppHandle,
    store: &ProxyStore,
    registry: &FlowWatchRegistry,
    sandbox: &str,
    site: &SidecarSite,
) {
    let mut tasks = registry.0.lock().unwrap();
    let Some(watch) = tasks.get_mut(sandbox) else {
        return;
    };
    watch.handle.abort();
    watch.handle = tauri::async_runtime::spawn(watch_flows(
        app.clone(),
        store.clone(),
        site.clone(),
        sandbox.to_string(),
    ));
}

/// Counts one watcher of `sandbox` gone and stops its poll task with the last one: for an iron-proxy
/// sidecar every tick spawns a `podman logs` process, which must not outlive every open panel.
/// Rows written meanwhile stay in the log, and the next watch replays them.
pub fn stop_watch(registry: &FlowWatchRegistry, sandbox: &str) {
    let mut tasks = registry.0.lock().unwrap();
    let Some(watch) = tasks.get_mut(sandbox) else {
        return;
    };
    watch.watchers = watch.watchers.saturating_sub(1);
    if watch.watchers == 0 {
        watch.handle.abort();
        tasks.remove(sandbox);
    }
}

/// Emits every newly appended flow until the log disappears, which is how proxy teardown is noticed
/// (`02_dev.md`, Teardown), or until [`stop_watch`] aborts it.
async fn watch_flows(app: AppHandle, store: ProxyStore, site: SidecarSite, sandbox: String) {
    let Ok(path) = store.flows_path(&sandbox) else {
        return;
    };
    let engine = store.sidecar_engine(&sandbox);
    let sidecar = super::proxy_container_name(&sandbox);
    let mut offset: u64 = 0;
    let mut last_id: u64 = 0;
    loop {
        tokio::time::sleep(FLOW_LOG_POLL_INTERVAL).await;

        let ingest_rewrote = match engine {
            ProxyEngine::IronProxy => match path.parent() {
                Some(dir) => {
                    // One cursor per site: the remote sidecar's log starts empty.
                    let cursor = match site.local {
                        true => dir.join(super::audit::CURSOR_FILE),
                        false => dir.join("remote").join(super::audit::CURSOR_FILE),
                    };
                    super::audit::ingest(site.provider.as_ref(), &sidecar, dir, &cursor).await
                }
                None => false,
            },
        };
        let len = match std::fs::metadata(&path) {
            Ok(meta) => meta.len(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
            Err(_) => continue,
        };
        // A rewrite past the cap invalidates the offset: re-read from the start and drop ids already
        // emitted, which are monotonic within one log. The ingest reports its rewrite, since the
        // rewritten file can be longer than the offset.
        let rewound = len < offset || ingest_rewrote;
        if rewound {
            offset = 0;
        }
        if len == offset {
            continue;
        }
        let Some((chunk, consumed)) = read_complete_lines(&path, offset) else {
            continue;
        };
        offset += consumed;

        let mut entries: Vec<FlowEntry> = chunk
            .lines()
            .filter_map(|line| serde_json::from_str::<FlowEntry>(line).ok())
            .filter(|entry| !rewound || entry.id > last_id)
            .collect();
        if entries.len() > FLOW_LOG_MAX_LINES {
            entries.drain(..entries.len() - FLOW_LOG_MAX_LINES);
        }
        let Some(newest) = entries.last() else {
            continue;
        };
        last_id = newest.id;
        let _ = app.emit(EVENT_NAME, serde_json::json!({ "sandbox": sandbox, "flows": entries }));
    }
}

/// Reads from `offset` to EOF, stopping at the last newline: a tick that catches the addon
/// mid-append must leave the partial trailing line for the next tick rather than dropping it.
fn read_complete_lines(path: &std::path::Path, offset: u64) -> Option<(String, u64)> {
    let mut file = std::fs::File::open(path).ok()?;
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).ok()?;
    let end = buf.iter().rposition(|b| *b == b'\n')? + 1;
    buf.truncate(end);
    Some((String::from_utf8_lossy(&buf).into_owned(), end as u64))
}
