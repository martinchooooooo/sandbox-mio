//! Poll loop over the proxy sidecar's NDJSON flow log
//! (`docs/features/proxy-traffic-viewer/02_dev.md`). One detached task per watched sandbox parses
//! only newly appended lines and pushes them to the frontend as `proxy://flows-changed`. A mitmproxy
//! sidecar's addon writes the file; for an iron-proxy sidecar each tick first ingests the audit
//! stream into it ([`super::audit`]).

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Emitter};

use crate::provider::ProviderRegistry;

const EVENT_NAME: &str = "proxy://flows-changed";

/// Cap on retained flows. Must equal `FLOW_LOG_MAX_LINES` in
/// `src-tauri/resources/mitm_flowlog_addon.py` and `src/lib/proxy/traffic.svelte.ts`.
pub const FLOW_LOG_MAX_LINES: usize = 500;

/// Matches `PORTS_FILE_POLL_INTERVAL` (`src-tauri/src/network/mod.rs`): a bind-mounted file read is
/// cheap enough to repeat this often, and frequent enough that the panel doesn't feel laggy
/// (Resolved Decision 6).
const FLOW_LOG_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// One finished flow as written by `mitm_flowlog_addon.py` or translated from iron-proxy's audit
/// stream. Metadata only — never headers, bodies,
/// or an injected secret value (`docs/features/proxy-traffic-viewer/01_ideas.md`, Capture Scope).
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

/// One poll task per watched sandbox, so a repeated `watch_proxy_flows` call is a no-op.
#[derive(Default)]
pub struct FlowWatchRegistry(Mutex<HashMap<String, tauri::async_runtime::JoinHandle<()>>>);

/// Starts `sandbox`'s poll task unless a live one is already registered; returns whether a new task
/// was spawned. Check and insert happen under one lock — two rapid calls would otherwise both spawn
/// and double every row.
///
/// Spawns through `tauri::async_runtime`, not `tokio::spawn`: a caller on the IPC thread has no
/// Tokio runtime entered, and `tokio::spawn`'s panic there cannot unwind across the webview's FFI
/// boundary — it aborts the process.
pub fn start_watch(
    app: &AppHandle,
    registry: &FlowWatchRegistry,
    providers: &Arc<ProviderRegistry>,
    sandbox: &str,
) -> bool {
    let mut tasks = registry.0.lock().unwrap();
    if tasks.get(sandbox).is_some_and(|handle| !handle.inner().is_finished()) {
        return false;
    }
    let handle = tauri::async_runtime::spawn(watch_flows(
        app.clone(),
        providers.clone(),
        sandbox.to_string(),
    ));
    tasks.insert(sandbox.to_string(), handle);
    true
}

/// Emits every newly appended flow until the log disappears, which is how proxy teardown is noticed
/// — there is no explicit cancel (`02_dev.md`, Teardown).
async fn watch_flows(app: AppHandle, providers: Arc<ProviderRegistry>, sandbox: String) {
    let Ok(path) = super::flows_path(&app, &sandbox) else {
        return;
    };
    let iron = super::read_meta(&app, &sandbox).is_some_and(|m| m["engine"] == super::ENGINE_IRON_PROXY);
    let sidecar = super::proxy_container_name(&sandbox);
    let mut offset: u64 = 0;
    let mut last_id: u64 = 0;
    loop {
        tokio::time::sleep(FLOW_LOG_POLL_INTERVAL).await;

        let ingest_rewrote = match (iron, path.parent()) {
            (true, Some(dir)) => {
                let (_, provider) = super::sidecar_provider(&app, &providers, &sandbox);
                super::audit::ingest(provider.as_ref(), &sidecar, dir).await
            }
            _ => false,
        };
        let len = match std::fs::metadata(&path) {
            Ok(meta) => meta.len(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
            Err(_) => continue,
        };
        // The addon or the audit ingest rewrites the file once it passes the cap, which invalidates
        // the offset: re-read from the start and drop the ids already emitted. Ids are monotonic
        // within one log, so `last_id` is only meaningful on this path. The ingest reports its
        // rewrite, since the rewritten file can be longer than the offset.
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
