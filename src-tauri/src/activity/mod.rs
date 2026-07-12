//! Global activity subsystem: TaskRegistry holding long-running task state and mutation helpers.

pub mod types;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Emitter};

use types::{Task, TaskId, TaskKind, TaskStatus};

static ACTIVITY_VERSION: AtomicU64 = AtomicU64::new(0);

const MAX_TERMINAL_TASKS: usize = 50;

pub const EVENT_NAME: &str = "activity://task-updated";

#[derive(Default)]
pub struct ActivityState {
    tasks: Mutex<HashMap<TaskId, Task>>,
}

pub type TaskRegistry = std::sync::Arc<ActivityState>;

impl ActivityState {
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

fn evict_terminal(map: &mut HashMap<TaskId, Task>) {
    let mut terminal: Vec<(TaskId, u64)> = map
        .iter()
        .filter(|(_, t)| t.status.is_terminal())
        .map(|(id, t)| (id.clone(), t.ended_at_ms.unwrap_or(0)))
        .collect();
    if terminal.len() <= MAX_TERMINAL_TASKS {
        return;
    }
    terminal.sort_by(|a, b| b.1.cmp(&a.1));
    for (id, _) in terminal.into_iter().skip(MAX_TERMINAL_TASKS) {
        map.remove(&id);
    }
}

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
