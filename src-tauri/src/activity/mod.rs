//! Global activity subsystem: the authoritative `TaskRegistry` (Tauri state, managed in `lib.rs`
//! `setup` alongside `ActiveServer`) plus the `open`/`progress`/`succeed`/`fail` helpers that
//! enrolled commands call. Each helper mutates the map under the lock, stamps the task with the
//! next process-wide version, clones it out, drops the lock, and only then broadcasts the whole
//! task with `app.emit(EVENT_NAME, &task)`. In-memory only: dropped on exit like `ActiveServer`.

pub mod types;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Emitter};

use types::{Task, TaskId, TaskKind, TaskStatus};

/// Process-wide monotonic version source, mirroring `SESSION_COUNTER` in `agents/mod.rs`. Bumped
/// under the registry lock on every mutation so version order matches insertion order.
static ACTIVITY_VERSION: AtomicU64 = AtomicU64::new(0);

/// Keep at most this many terminal tasks; `running` tasks are never evicted.
const MAX_TERMINAL_TASKS: usize = 50;

/// The single app-wide event both producers (here) and the consumer (`activity.svelte.ts`) name.
pub const EVENT_NAME: &str = "activity://task-updated";

#[derive(Default)]
pub struct ActivityState {
    tasks: Mutex<HashMap<TaskId, Task>>,
}

pub type TaskRegistry = std::sync::Arc<ActivityState>;

impl ActivityState {
    /// Snapshot the current ledger, sorted by `started_at_ms`. Infallible — a read cannot fail.
    pub fn snapshot(&self) -> Vec<Task> {
        let mut tasks: Vec<Task> = self.tasks.lock().unwrap().values().cloned().collect();
        tasks.sort_by_key(|t| t.started_at_ms);
        tasks
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

fn next_version() -> u64 {
    ACTIVITY_VERSION.fetch_add(1, Ordering::Relaxed)
}

/// Drop terminal tasks beyond the newest `MAX_TERMINAL_TASKS` (by `ended_at_ms`); keep all running.
fn evict_terminal(map: &mut HashMap<TaskId, Task>) {
    let mut terminal: Vec<(TaskId, u64)> = map
        .iter()
        .filter(|(_, t)| t.status.is_terminal())
        .map(|(id, t)| (id.clone(), t.ended_at_ms.unwrap_or(0)))
        .collect();
    if terminal.len() <= MAX_TERMINAL_TASKS {
        return;
    }
    // Newest first; drop everything past the cap.
    terminal.sort_by(|a, b| b.1.cmp(&a.1));
    for (id, _) in terminal.into_iter().skip(MAX_TERMINAL_TASKS) {
        map.remove(&id);
    }
}

/// Resolve the registry from the app handle and run `f` with the stamped task cloned out, then
/// emit it outside the lock. A best-effort emit (a dropped emit never fails the underlying work).
fn mutate(app: &AppHandle, f: impl FnOnce(&mut HashMap<TaskId, Task>) -> Option<Task>) {
    use tauri::Manager;
    let registry = app.state::<TaskRegistry>();
    let task = {
        let mut map = registry.tasks.lock().unwrap();
        f(&mut map)
    };
    if let Some(task) = task {
        app.emit(EVENT_NAME, &task).ok();
    }
}

/// Open (or replace) a `Running` task with a fresh version and `started_at_ms`, then emit.
/// Replacing an existing id by natural key is intentional for singletons (e.g. `inference-setup`).
pub fn open(app: &AppHandle, id: &str, kind: TaskKind, target: Option<String>) {
    mutate(app, |map| {
        let task = Task {
            id: id.to_string(),
            kind,
            version: next_version(),
            status: TaskStatus::Running,
            progress: None,
            target,
            detail: None,
            error: None,
            started_at_ms: now_ms(),
            ended_at_ms: None,
        };
        map.insert(id.to_string(), task.clone());
        Some(task)
    });
}

/// Update `progress`/`detail` of a running task, bump version, emit. No-op if the id is absent.
pub fn progress(app: &AppHandle, id: &str, fraction: Option<f32>, detail: Option<String>) {
    mutate(app, |map| {
        let task = map.get_mut(id)?;
        task.version = next_version();
        if fraction.is_some() {
            task.progress = fraction;
        }
        if detail.is_some() {
            task.detail = detail;
        }
        Some(task.clone())
    });
}

/// Mark a task `Succeeded`, bump version, emit, then evict old terminal tasks. No-op if absent.
pub fn succeed(app: &AppHandle, id: &str, detail: Option<String>) {
    mutate(app, |map| {
        let task = map.get_mut(id)?;
        task.version = next_version();
        task.status = TaskStatus::Succeeded;
        task.progress = Some(1.0);
        if detail.is_some() {
            task.detail = detail;
        }
        task.ended_at_ms = Some(now_ms());
        let updated = task.clone();
        evict_terminal(map);
        Some(updated)
    });
}

/// Mark a task `Failed` with `error`, bump version, emit, then evict old terminal tasks.
pub fn fail(app: &AppHandle, id: &str, error: String) {
    mutate(app, |map| {
        let task = map.get_mut(id)?;
        task.version = next_version();
        task.status = TaskStatus::Failed;
        task.error = Some(error);
        task.ended_at_ms = Some(now_ms());
        let updated = task.clone();
        evict_terminal(map);
        Some(updated)
    });
}
