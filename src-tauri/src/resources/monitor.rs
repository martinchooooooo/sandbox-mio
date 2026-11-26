//! The sampling task and the latest snapshot: one tick lists the local sandboxes, reads each running
//! engine's stats, derives CPU fractions, and emits the snapshot.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};

use super::history::{
    append_line, pool_file, prune_dir, prune_file, sandbox_file, PRUNE_GROWTH_BYTES,
};
use super::rollup::{mib, MinuteAcc, PoolRollup, ResourceSample, Rollup};
use super::unix_now;
use mio_domain::types::{ProviderId, Sandbox};
use mio_provider::types::{EnginePool, InstanceStats};
use mio_provider::ProviderRegistry;

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

/// Whether an engine instance is a sandbox the user manages. The caller decides, because telling a
/// sandbox from the app's own sidecars and auth containers takes subsystems this one may not name.
pub type SandboxFilter = fn(&Sandbox) -> bool;

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

impl ResourceMonitor {
    /// Starts the sampling task. Idempotent. `is_user_sandbox` keeps sidecars and auth containers
    /// out of the monitored set.
    pub fn start(
        self: &Arc<Self>,
        app: AppHandle,
        registry: Arc<ProviderRegistry>,
        is_user_sandbox: SandboxFilter,
    ) {
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
                monitor.tick(&app, &registry, is_user_sandbox).await;
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
        if mio_domain::validate_name(name).is_err() {
            return;
        }
        if let Ok(dir) = crate::paths::resource_history_dir(app) {
            let path = sandbox_file(&dir, name);
            if let Err(e) = std::fs::remove_file(&path) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(target: "resources", %name, error = %e, "could not remove the resource history");
                }
            }
            self.state.lock().unwrap().pruned_size.remove(&path);
        }
    }

    async fn tick(
        &self,
        app: &AppHandle,
        registry: &Arc<ProviderRegistry>,
        is_user_sandbox: SandboxFilter,
    ) {
        let refresh = {
            let state = self.state.lock().unwrap();
            state.known_at.is_none_or(|at| at.elapsed() >= NAME_REFRESH)
        };
        if refresh {
            let known = known_sandboxes(registry, is_user_sandbox).await;
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
            if !is_running || (id == ProviderId::Podman && mio_process::podman::podman_breaker_open()) {
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
            if mio_domain::validate_name(&name).is_err() {
                continue;
            }
            if let Ok(line) = serde_json::to_string(&rollup) {
                lines.push((sandbox_file(&dir, &name), line));
            }
        }
        if let Some(pool) = pool {
            if let Ok(line) = serde_json::to_string(&pool) {
                lines.push((pool_file(&dir), line));
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
async fn known_sandboxes(
    registry: &Arc<ProviderRegistry>,
    is_user_sandbox: SandboxFilter,
) -> HashMap<String, (ProviderId, bool)> {
    let mut known = HashMap::new();
    for (id, provider) in registry.all() {
        match provider.list_instances().await {
            Ok(instances) => {
                for s in instances {
                    if !is_user_sandbox(&s) {
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
}
