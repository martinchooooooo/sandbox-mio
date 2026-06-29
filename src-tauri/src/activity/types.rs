//! Types for the global activity subsystem: the cross-kind `Task` envelope broadcast over
//! `activity://task-updated` and returned by `list_tasks`. No prose lives here — the human-facing
//! label is derived frontend-side from `kind` via Paraglide; `target`/`detail`/`error` are
//! verbatim backend facts shown as-is (mirroring `RuntimeSetupEvent.detail`).

pub type TaskId = String;

#[derive(serde::Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TaskKind {
    ModelDownload,
    InferenceSetup,
    HostProvision,
    AgentInstall,
}

#[derive(serde::Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Running,
    Succeeded,
    Failed,
}

impl TaskStatus {
    pub fn is_terminal(self) -> bool {
        !matches!(self, TaskStatus::Running)
    }
}

/// One long-running background operation. Broadcast verbatim over `activity://task-updated` and
/// returned by `list_tasks`. The `version` stamp is a process-wide monotonic counter the frontend
/// folds by strictly-greater value so the snapshot and the live stream are commutative.
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: TaskId,
    pub kind: TaskKind,
    pub version: u64,
    pub status: TaskStatus,
    /// 0.0..=1.0; `None` = indeterminate (step-based work without a fraction).
    pub progress: Option<f32>,
    /// Verbatim subject shown next to the translated kind label, e.g. a model id.
    pub target: Option<String>,
    /// Latest verbatim backend fact, e.g. "1.4 / 2.5 GB" or a setup step id.
    pub detail: Option<String>,
    /// Set only on `Failed`; the verbatim error message.
    pub error: Option<String>,
    pub started_at_ms: u64,
    pub ended_at_ms: Option<u64>,
}
