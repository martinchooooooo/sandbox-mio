//! The resource monitor (`docs/features/sandbox-resource-monitor/02_dev.md`): one background task
//! samples every running local sandbox's CPU and memory each tick, pushes the live snapshot to the
//! frontend as `resources://snapshot`, and appends one-minute rollups to a JSON-lines history file
//! per sandbox under `paths::resource_history_dir`.

use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Emitter};

use crate::domain::types::ProviderId;
use crate::provider::types::{EnginePool, InstanceStats};
use crate::provider::ProviderRegistry;

pub const SNAPSHOT_EVENT: &str = "resources://snapshot";

const TICK: Duration = Duration::from_secs(30);

/// Ticks per one-minute rollup and pool reading.
const TICKS_PER_ROLLUP: u32 = 2;

const NAME_REFRESH: Duration = Duration::from_secs(60);

/// Live samples kept per sandbox. Must cover the three-sample window `src/lib/sandbox/health.ts`
/// classifies over.
const RING_LEN: usize = 6;

/// A sandbox whose engine call keeps failing keeps its last samples for this many ticks.
const MAX_MISSED_TICKS: u32 = 3;

/// The share of the limit a sample counts as under memory pressure in the rollups. Equal to
/// `STRAINED_MEMORY` in `src/lib/sandbox/health.ts`.
const PRESSURE_FRACTION: f64 = 0.9;

const RETENTION_SECS: u64 = 14 * 24 * 3600;

/// A history file is pruned at monitor start and whenever it has grown this much since.
const PRUNE_GROWTH_BYTES: u64 = 512 * 1024;

/// The Podman machine's history file stem. A leading underscore cannot collide with a sandbox,
/// whose name starts with an ASCII alphanumeric (`validate_name` in `src-tauri/src/domain/mod.rs`).
const POOL_FILE_STEM: &str = "_podman-machine";

/// One live reading of one sandbox.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceSample {
    pub at_ms: u64,
    /// Fraction (0 to 1) of the allotted CPUs used since the previous sample; `None` for the first
    /// sample after start, or while the allotted CPU count is unknown.
    pub cpu: Option<f64>,
    pub memory_used: u64,
    pub memory_limit: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxResources {
    pub name: String,
    pub provider: ProviderId,
    pub allotted_cpus: Option<u32>,
    pub memory_includes_cache: bool,
    /// Oldest first, at most [`RING_LEN`].
    pub samples: Vec<ResourceSample>,
}

/// Every running local sandbox's recent samples plus the Podman machine, as last sampled.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceSnapshot {
    pub at_ms: u64,
    pub sandboxes: Vec<SandboxResources>,
    pub podman_pool: Option<EnginePool>,
}

/// One minute of one sandbox, as written to its history file. Memory is in MiB and CPU is rounded
/// to [`CPU_DECIMALS`] places, which keeps a line near 110 bytes.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rollup {
    /// Unix seconds at the end of the minute.
    pub ts: u64,
    pub provider: ProviderId,
    /// Mean CPU fraction; absent when no sample in the minute had a CPU reading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_max: Option<f64>,
    /// Mean memory in use.
    pub mem: u64,
    pub mem_max: u64,
    pub limit: u64,
    /// Samples at or above [`PRESSURE_FRACTION`] of the limit.
    pub high: u32,
    /// Which memory definition the provider reported (Resolved Decision 5); written only when true.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub includes_cache: bool,
}

/// One minute of the Podman machine, as written to `_podman-machine.jsonl`, memory in MiB.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolRollup {
    pub ts: u64,
    pub cpus: u32,
    pub total: u64,
    /// The kernel's free memory, which counts page cache as used.
    pub free: u64,
}

const CPU_DECIMALS: i32 = 4;

fn round_cpu(fraction: f64) -> f64 {
    let scale = 10f64.powi(CPU_DECIMALS);
    (fraction * scale).round() / scale
}

fn mib(bytes: u64) -> u64 {
    (bytes + (1 << 19)) >> 20
}

#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HistoryWindow {
    Day,
    Week,
}

impl HistoryWindow {
    fn secs(self) -> u64 {
        match self {
            HistoryWindow::Day => 24 * 3600,
            HistoryWindow::Week => 7 * 24 * 3600,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxHistory {
    pub name: String,
    pub rollups: Vec<Rollup>,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceHistory {
    pub sandboxes: Vec<SandboxHistory>,
    pub podman_pool: Vec<PoolRollup>,
}

#[derive(Default)]
struct MinuteAcc {
    cpu_sum: f64,
    cpu_n: u32,
    cpu_max: f64,
    mem_sum: u64,
    mem_max: u64,
    mem_limit: u64,
    n: u32,
    high: u32,
}

impl MinuteAcc {
    fn add(&mut self, sample: &ResourceSample) {
        if let Some(cpu) = sample.cpu {
            self.cpu_sum += cpu;
            self.cpu_n += 1;
            self.cpu_max = self.cpu_max.max(cpu);
        }
        self.mem_sum += sample.memory_used;
        self.mem_max = self.mem_max.max(sample.memory_used);
        self.mem_limit = sample.memory_limit;
        self.n += 1;
        if sample.memory_limit > 0
            && sample.memory_used as f64 >= PRESSURE_FRACTION * sample.memory_limit as f64
        {
            self.high += 1;
        }
    }

    /// The minute's rollup, resetting the accumulator; `None` for a minute without samples.
    fn take(&mut self, ts: u64, provider: ProviderId, includes_cache: bool) -> Option<Rollup> {
        let acc = std::mem::take(self);
        (acc.n > 0).then(|| Rollup {
            ts,
            provider,
            cpu: (acc.cpu_n > 0).then(|| round_cpu(acc.cpu_sum / acc.cpu_n as f64)),
            cpu_max: (acc.cpu_n > 0).then(|| round_cpu(acc.cpu_max)),
            mem: mib(acc.mem_sum / acc.n as u64),
            mem_max: mib(acc.mem_max),
            limit: mib(acc.mem_limit),
            high: acc.high,
            includes_cache,
        })
    }
}

struct Tracked {
    provider: ProviderId,
    allotted_cpus: Option<u32>,
    includes_cache: bool,
    ring: VecDeque<ResourceSample>,
    /// The previous reading's cumulative CPU time and engine clock.
    last_raw: Option<(u64, u64)>,
    missed: u32,
    minute: MinuteAcc,
}

#[derive(Default)]
struct State {
    /// Sandbox name to its provider and whether it runs; sidecars and auth containers excluded.
    known: HashMap<String, (ProviderId, bool)>,
    known_at: Option<Instant>,
    tracked: HashMap<String, Tracked>,
    pool: Option<EnginePool>,
    tick: u32,
    snapshot: ResourceSnapshot,
    /// Each history file's size after its last prune.
    pruned_size: HashMap<PathBuf, u64>,
}

/// Owns the sampling task and the latest snapshot. Managed as `Arc<ResourceMonitor>`.
#[derive(Default)]
pub struct ResourceMonitor {
    state: Mutex<State>,
    task: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
}

/// The CPU fraction between two cumulative readings, or `None` when the counter went backwards (the
/// instance restarted) or the engine clock did not advance.
fn cpu_fraction(prev: (u64, u64), now: (u64, u64), allotted: u32) -> Option<f64> {
    let (cpu0, at0) = prev;
    let (cpu1, at1) = now;
    if allotted == 0 || cpu1 < cpu0 || at1 <= at0 {
        return None;
    }
    Some(((cpu1 - cpu0) as f64 / (at1 - at0) as f64 / allotted as f64).clamp(0.0, 1.0))
}

fn unix_now() -> Duration {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default()
}

impl ResourceMonitor {
    /// Starts the sampling task. Idempotent.
    pub fn start(self: &Arc<Self>, app: AppHandle, registry: Arc<ProviderRegistry>) {
        let mut task = self.task.lock().unwrap();
        if task.is_some() {
            return;
        }
        let monitor = self.clone();
        *task = Some(tauri::async_runtime::spawn(async move {
            if let Ok(dir) = crate::paths::resource_history_dir(&app) {
                prune_dir(&dir);
            }
            let mut interval = tokio::time::interval(TICK);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                monitor.tick(&app, &registry).await;
            }
        }));
    }

    /// Stops the sampling task, so no engine call is in flight at exit.
    pub fn stop(&self) {
        if let Some(task) = self.task.lock().unwrap().take() {
            task.abort();
        }
    }

    pub fn snapshot(&self) -> ResourceSnapshot {
        self.state.lock().unwrap().snapshot.clone()
    }

    /// Drops a destroyed sandbox from the live snapshot and deletes its history file.
    pub fn forget(&self, app: &AppHandle, name: &str) {
        {
            let mut state = self.state.lock().unwrap();
            state.tracked.remove(name);
            state.known.remove(name);
            state.snapshot.sandboxes.retain(|s| s.name != name);
        }
        if crate::domain::validate_name(name).is_err() {
            return;
        }
        if let Ok(dir) = crate::paths::resource_history_dir(app) {
            let path = dir.join(format!("{name}.jsonl"));
            if let Err(e) = std::fs::remove_file(&path) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(target: "resources", %name, error = %e, "could not remove the resource history");
                }
            }
            self.state.lock().unwrap().pruned_size.remove(&path);
        }
    }

    async fn tick(&self, app: &AppHandle, registry: &Arc<ProviderRegistry>) {
        let refresh = {
            let state = self.state.lock().unwrap();
            state.known_at.is_none_or(|at| at.elapsed() >= NAME_REFRESH)
        };
        if refresh {
            let known = known_sandboxes(registry).await;
            let mut state = self.state.lock().unwrap();
            state.known = known;
            state.known_at = Some(Instant::now());
        }
        let (running, rollup_due, need_pool) = {
            let mut state = self.state.lock().unwrap();
            state.tick = state.tick.wrapping_add(1);
            let running = |p: ProviderId| state.known.values().any(|&(id, run)| id == p && run);
            let running = [ProviderId::Podman].map(|p| (p, running(p)));
            let rollup_due = state.tick % TICKS_PER_ROLLUP == 0;
            (running, rollup_due, state.pool.is_none() || rollup_due)
        };

        let mut readings: Vec<(ProviderId, Option<Vec<InstanceStats>>)> = Vec::new();
        let mut pool_reading = None;
        for (id, is_running) in running {
            // Skipping while the breaker is open keeps a struggling engine from being polled at
            // all (docs/features/sandbox-resource-monitor/02_dev.md, Resolved Decision 8).
            if !is_running || (id == ProviderId::Podman && crate::process::podman_breaker_open()) {
                continue;
            }
            let provider = registry.local(id);
            let stats = match provider.instance_stats().await {
                Ok(stats) => Some(stats),
                Err(e) => {
                    tracing::debug!(target: "resources", provider = %id, error = %e, "stats call failed");
                    None
                }
            };
            readings.push((id, stats));
            if id == ProviderId::Podman && need_pool {
                match provider.engine_pool().await {
                    Ok(pool) => pool_reading = pool,
                    Err(e) => {
                        tracing::debug!(target: "resources", error = %e, "pool call failed")
                    }
                }
            }
        }

        let now = unix_now();
        let at_ms = now.as_millis() as u64;
        let (snapshot, rollups, pool_rollup) = {
            let mut state = self.state.lock().unwrap();
            let state = &mut *state;
            if pool_reading.is_some() {
                state.pool = pool_reading;
            }
            let sampled: Vec<ProviderId> = readings.iter().map(|(id, _)| *id).collect();
            // An engine not called has no running sandbox, so its sandboxes leave the snapshot.
            state.tracked.retain(|_, t| sampled.contains(&t.provider));
            let mut unknown_name = false;
            for (id, stats) in readings {
                let Some(stats) = stats else {
                    state.tracked.retain(|_, t| {
                        if t.provider == id {
                            t.missed += 1;
                        }
                        t.provider != id || t.missed < MAX_MISSED_TICKS
                    });
                    continue;
                };
                let mut seen = Vec::new();
                for s in stats {
                    match state.known.get(&s.name) {
                        Some(&(known_id, _)) if known_id == id => {}
                        Some(_) => continue,
                        None => {
                            // A sidecar is also absent from the set, so it is dropped here; a
                            // genuinely new sandbox is picked up by the next refresh.
                            unknown_name = true;
                            continue;
                        }
                    }
                    let allotted = s.allotted_cpus.or(match id {
                        ProviderId::Podman => state.pool.map(|p| p.cpus),
                    });
                    let t = state.tracked.entry(s.name.clone()).or_insert_with(|| Tracked {
                        provider: id,
                        allotted_cpus: allotted,
                        includes_cache: s.memory_includes_cache,
                        ring: VecDeque::with_capacity(RING_LEN),
                        last_raw: None,
                        missed: 0,
                        minute: MinuteAcc::default(),
                    });
                    let raw = (s.cpu_time_ns, s.sampled_at_ns);
                    let cpu = t.last_raw.zip(allotted).and_then(|(prev, n)| cpu_fraction(prev, raw, n));
                    let sample = ResourceSample {
                        at_ms,
                        cpu,
                        memory_used: s.memory_used,
                        memory_limit: s.memory_limit,
                    };
                    t.minute.add(&sample);
                    if t.ring.len() == RING_LEN {
                        t.ring.pop_front();
                    }
                    t.ring.push_back(sample);
                    t.last_raw = Some(raw);
                    t.allotted_cpus = allotted;
                    t.includes_cache = s.memory_includes_cache;
                    t.missed = 0;
                    seen.push(s.name);
                }
                // A sandbox missing from a successful reading has stopped.
                state.tracked.retain(|name, t| t.provider != id || seen.contains(name));
            }
            if unknown_name {
                state.known_at = None;
            }

            let ts = now.as_secs();
            let mut rollups = Vec::new();
            let mut pool_rollup = None;
            if rollup_due {
                for (name, t) in state.tracked.iter_mut() {
                    if let Some(r) = t.minute.take(ts, t.provider, t.includes_cache) {
                        rollups.push((name.clone(), r));
                    }
                }
                if sampled.contains(&ProviderId::Podman) {
                    pool_rollup = state.pool.map(|p| PoolRollup {
                        ts,
                        cpus: p.cpus,
                        total: mib(p.memory_total),
                        free: mib(p.memory_free),
                    });
                }
            }

            let mut sandboxes: Vec<SandboxResources> = state
                .tracked
                .iter()
                .map(|(name, t)| SandboxResources {
                    name: name.clone(),
                    provider: t.provider,
                    allotted_cpus: t.allotted_cpus,
                    memory_includes_cache: t.includes_cache,
                    samples: t.ring.iter().cloned().collect(),
                })
                .collect();
            sandboxes.sort_by(|a, b| a.name.cmp(&b.name));
            let podman_pool = if sampled.contains(&ProviderId::Podman) { state.pool } else { None };
            state.snapshot = ResourceSnapshot { at_ms, sandboxes, podman_pool };
            (state.snapshot.clone(), rollups, pool_rollup)
        };

        if !rollups.is_empty() || pool_rollup.is_some() {
            self.write_rollups(app, rollups, pool_rollup);
        }
        if let Err(e) = app.emit(SNAPSHOT_EVENT, &snapshot) {
            tracing::debug!(target: "resources", error = %e, "could not emit the snapshot");
        }
    }

    fn write_rollups(
        &self,
        app: &AppHandle,
        rollups: Vec<(String, Rollup)>,
        pool: Option<PoolRollup>,
    ) {
        let Ok(dir) = crate::paths::resource_history_dir(app) else { return };
        if let Err(e) = std::fs::create_dir_all(&dir) {
            tracing::warn!(target: "resources", error = %e, "could not create the resource history directory");
            return;
        }
        let mut lines: Vec<(PathBuf, String)> = Vec::new();
        for (name, rollup) in rollups {
            if crate::domain::validate_name(&name).is_err() {
                continue;
            }
            if let Ok(line) = serde_json::to_string(&rollup) {
                lines.push((dir.join(format!("{name}.jsonl")), line));
            }
        }
        if let Some(pool) = pool {
            if let Ok(line) = serde_json::to_string(&pool) {
                lines.push((dir.join(format!("{POOL_FILE_STEM}.jsonl")), line));
            }
        }
        for (path, line) in lines {
            if let Err(e) = append_line(&path, &line) {
                tracing::warn!(target: "resources", path = %path.display(), error = %e, "could not append to the resource history");
                continue;
            }
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let mut state = self.state.lock().unwrap();
            let base = *state.pruned_size.entry(path.clone()).or_insert(0);
            if size > base + PRUNE_GROWTH_BYTES {
                prune_file(&path);
                let after = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                state.pruned_size.insert(path, after);
            }
        }
    }
}

/// Every local sandbox by name with its provider and running state, asking each provider on its
/// own, so one provider's stopped machine does not hide another's sandboxes.
async fn known_sandboxes(registry: &Arc<ProviderRegistry>) -> HashMap<String, (ProviderId, bool)> {
    let mut known = HashMap::new();
    for (id, provider) in registry.all() {
        match provider.list_instances().await {
            Ok(instances) => {
                for s in instances {
                    if s.name.starts_with(crate::credentials::types::AUTH_CONTAINER_PREFIX)
                        || crate::proxy::is_sidecar(&s)
                    {
                        continue;
                    }
                    let running = s.status == "running";
                    known.insert(s.name, (id, running));
                }
            }
            Err(e) => {
                tracing::debug!(target: "resources", provider = %id, error = %e, "list failed")
            }
        }
    }
    known
}

fn append_line(path: &Path, line: &str) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{line}")
}

#[derive(serde::Deserialize)]
struct Stamped {
    ts: u64,
}

/// The lines of `text` stamped at or after `cutoff`; unreadable lines are dropped.
fn lines_since(text: &str, cutoff: u64) -> Vec<&str> {
    text.lines()
        .filter(|l| serde_json::from_str::<Stamped>(l).is_ok_and(|s| s.ts >= cutoff))
        .collect()
}

fn prune_file(path: &Path) {
    let Ok(text) = std::fs::read_to_string(path) else { return };
    let cutoff = unix_now().as_secs().saturating_sub(RETENTION_SECS);
    let kept = lines_since(&text, cutoff);
    let mut body = kept.join("\n");
    if !body.is_empty() {
        body.push('\n');
    }
    let tmp = path.with_extension("jsonl.tmp");
    let written = std::fs::write(&tmp, body).and_then(|()| std::fs::rename(&tmp, path));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        tracing::warn!(target: "resources", path = %path.display(), error = %e, "could not prune the resource history");
    }
}

fn prune_dir(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "jsonl") {
            prune_file(&path);
        }
    }
}

fn read_rollups<T: serde::de::DeserializeOwned>(path: &Path, cutoff: u64) -> std::io::Result<Vec<T>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    Ok(lines_since(&text, cutoff).into_iter().filter_map(|l| serde_json::from_str(l).ok()).collect())
}

/// The rollups of `name` over `window`, or of every sandbox and the Podman machine when `name` is
/// `None`.
pub fn history(
    app: &AppHandle,
    name: Option<&str>,
    window: HistoryWindow,
) -> Result<ResourceHistory, crate::error::AppError> {
    let dir = crate::paths::resource_history_dir(app)
        .map_err(|e| crate::domain::types::BackendError::CommandFailed(e.to_string()))?;
    let io = |e: std::io::Error| crate::domain::types::BackendError::CommandFailed(e.to_string());
    let cutoff = unix_now().as_secs().saturating_sub(window.secs());
    let mut history = ResourceHistory::default();
    match name {
        Some(name) => {
            crate::domain::validate_name(name)?;
            let rollups = read_rollups(&dir.join(format!("{name}.jsonl")), cutoff).map_err(io)?;
            history.sandboxes.push(SandboxHistory { name: name.to_string(), rollups });
        }
        None => {
            let entries = match std::fs::read_dir(&dir) {
                Ok(entries) => entries,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(history),
                Err(e) => return Err(io(e).into()),
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.extension().is_some_and(|e| e == "jsonl") {
                    continue;
                }
                let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
                if stem == POOL_FILE_STEM {
                    history.podman_pool = read_rollups(&path, cutoff).map_err(io)?;
                } else {
                    let rollups = read_rollups(&path, cutoff).map_err(io)?;
                    history.sandboxes.push(SandboxHistory { name: stem.to_string(), rollups });
                }
            }
            history.sandboxes.sort_by(|a, b| a.name.cmp(&b.name));
        }
    }
    Ok(history)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_fraction_divides_by_wall_time_and_allotted_cpus() {
        // Two CPUs busy for 5 s on an 8-CPU machine.
        let f = cpu_fraction((1_000, 0), (10_000_001_000, 5_000_000_000), 8).unwrap();
        assert!((f - 0.25).abs() < 1e-9, "{f}");
        assert_eq!(cpu_fraction((5, 0), (4, 10), 1), None, "a restart resets the counter");
        assert_eq!(cpu_fraction((0, 10), (5, 10), 1), None, "the clock did not advance");
        assert_eq!(cpu_fraction((0, 0), (20, 10), 1), Some(1.0), "clamped to the allotment");
    }

    const MIB: u64 = 1 << 20;

    fn sample(cpu: Option<f64>, used_mib: u64) -> ResourceSample {
        ResourceSample { at_ms: 0, cpu, memory_used: used_mib * MIB, memory_limit: 100 * MIB }
    }

    #[test]
    fn a_minute_rolls_up_means_peaks_and_pressure() {
        let mut acc = MinuteAcc::default();
        acc.add(&sample(None, 50));
        acc.add(&sample(Some(0.2), 90));
        acc.add(&sample(Some(0.4), 95));
        let r = acc.take(60, ProviderId::Podman, false).unwrap();
        assert_eq!(r.high, 2);
        assert_eq!(r.mem, 78);
        assert_eq!(r.mem_max, 95);
        assert_eq!(r.limit, 100);
        assert_eq!(r.cpu, Some(0.3));
        assert_eq!(r.cpu_max, Some(0.4));
        assert!(acc.take(120, ProviderId::Podman, false).is_none(), "the accumulator resets");
    }

    #[test]
    fn old_and_unreadable_lines_are_dropped() {
        let text = "{\"ts\":5}\nnot json\n{\"ts\":20,\"x\":1}\n";
        assert_eq!(lines_since(text, 10), ["{\"ts\":20,\"x\":1}"]);
    }

    #[test]
    fn rollups_round_trip_through_a_history_line() {
        let r = Rollup {
            ts: 1,
            provider: ProviderId::Podman,
            cpu: None,
            cpu_max: None,
            mem: 2,
            mem_max: 3,
            limit: 4,
            high: 0,
            includes_cache: false,
        };
        let line = serde_json::to_string(&r).unwrap();
        assert_eq!(line, r#"{"ts":1,"provider":"podman","mem":2,"memMax":3,"limit":4,"high":0}"#);
        assert_eq!(serde_json::from_str::<Rollup>(&line).unwrap(), r);
    }
}
