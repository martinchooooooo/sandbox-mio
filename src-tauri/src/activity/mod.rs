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

/// How a [`scope`] body fails: the message recorded on the task, and the error the command
/// propagates. The two are separate because they differ — an `AppError`'s `Display` prefixes its
/// variant ("Podman is not ready: …"), which the Activity Center row should not repeat.
pub struct TaskFailure<E> {
    message: String,
    pub error: E,
}

impl<E> TaskFailure<E> {
    pub fn new(message: impl Into<String>, error: E) -> Self {
        Self { message: message.into(), error }
    }
}

impl<E: std::fmt::Display> TaskFailure<E> {
    /// Records the error's own `Display` and propagates it unchanged.
    pub fn same(error: E) -> Self {
        Self { message: error.to_string(), error }
    }
}

/// A handle to the task an in-flight [`scope`] opened.
pub struct TaskScope {
    app: AppHandle,
    id: String,
    success_detail: std::sync::Arc<Mutex<Option<String>>>,
}

impl TaskScope {
    /// Reports a step, exactly as [`progress`] does.
    pub fn progress(&self, fraction: Option<f32>, detail: impl Into<String>) {
        progress(&self.app, &self.id, fraction, Some(detail.into()));
    }

    /// The detail to attach when the scope succeeds. Last call wins; unset leaves the detail as the
    /// last `progress` left it.
    pub fn set_success_detail(&self, detail: impl Into<String>) {
        *self.success_detail.lock().unwrap() = Some(detail.into());
    }
}

/// Opens a task, runs `body` with a handle to it, and resolves it from what `body` returns.
///
/// A closure rather than an RAII guard: `Drop` cannot see the error value, so a failed task would
/// lose the message the Activity Center displays
/// (`docs/features/lib-decomposition/01_ideas.md`, Activity Scope Helper Shape).
pub async fn scope<T, E, Fut>(
    app: &AppHandle,
    id: &str,
    kind: TaskKind,
    target: Option<String>,
    body: impl FnOnce(TaskScope) -> Fut,
) -> Result<T, E>
where
    Fut: std::future::Future<Output = Result<T, TaskFailure<E>>>,
{
    open(app, id, kind, target);
    let success_detail = std::sync::Arc::new(Mutex::new(None));
    let handle =
        TaskScope { app: app.clone(), id: id.to_string(), success_detail: success_detail.clone() };
    match body(handle).await {
        Ok(value) => {
            let detail = success_detail.lock().unwrap().take();
            succeed(app, id, detail);
            Ok(value)
        }
        Err(f) => {
            fail(app, id, f.message);
            Err(f.error)
        }
    }
}

/// Mirrors a stepped command's progress events to `downstream` while folding `stepDone` and
/// `stepSkipped` counts into a progress fraction on task `id`. The `ready` event is the terminal
/// one and is forwarded without a progress update.
pub fn step_progress_channel(
    app: &AppHandle,
    id: &str,
    total_steps: usize,
    downstream: tauri::ipc::Channel<serde_json::Value>,
) -> tauri::ipc::Channel<serde_json::Value> {
    let app = app.clone();
    let id = id.to_string();
    let completed = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    tauri::ipc::Channel::new(move |body| {
        if let Ok(val) = body.deserialize::<serde_json::Value>() {
            downstream.send(val.clone()).ok();
            let kind = val.get("kind").and_then(|k| k.as_str()).unwrap_or("");
            if matches!(kind, "stepDone" | "stepSkipped") {
                completed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            if kind != "ready" {
                let done = completed.load(std::sync::atomic::Ordering::Relaxed);
                let fraction = (total_steps > 0).then(|| done as f32 / total_steps as f32);
                let detail = val
                    .get("detail")
                    .and_then(|v| v.as_str())
                    .or_else(|| val.get("id").and_then(|v| v.as_str()))
                    .map(String::from);
                progress(&app, &id, fraction, detail);
            }
        }
        Ok(())
    })
}
