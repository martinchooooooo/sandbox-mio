//! Global activity center: long-running tasks broadcast over `activity://task-updated` and
//! returned by `list_tasks`. Human-readable labels are derived frontend-side from `kind` via Paraglide;
//! `target`/`detail`/`error` are verbatim backend facts displayed as-is.

pub type TaskId = String;

#[derive(serde::Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TaskKind {
    ModelDownload,
    InferenceSetup,
    /// Not yet emitted by any backend flow; frontend labels already exist in
    /// `src/lib/activity/activity.svelte.ts` awaiting this wiring.
    #[allow(dead_code)]
    HostProvision,
    #[allow(dead_code)]
    AgentInstall,
    PodmanStartup,
    /// The mitmproxy sidecar image's first pull (~200 MB), reported explicitly rather than left to
    /// look like a hung sandbox create (`docs/features/network-proxy/02_dev.md`, Resolved
    /// Decision 10).
    ProxyImagePull,
    /// Recreating a microsandbox sandbox to apply its network policy or viewer change
    /// (`docs/features/microsandbox-network-policy/02_dev.md`, Phase G).
    NetworkReapply,
    /// Bringing a provider's machine online from the welcome flow's checklist (`prepare_machine`).
    MachineSetup,
    /// Building the base image on its own, ahead of the first sandbox (`build_base_image`).
    BaseImageBuild,
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

/// One long-running background operation broadcast over `activity://task-updated`.
/// The `version` field is monotonically increasing; frontend folds by strictly-greater value.
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: TaskId,
    pub kind: TaskKind,
    /// Process-wide monotonic version for deduplication.
    pub version: u64,
    pub status: TaskStatus,
    /// Progress as fraction (0.0..=1.0), or `None` for step-based work.
    pub progress: Option<f32>,
    /// Subject shown beside the translated kind (e.g., model id, step id).
    pub target: Option<String>,
    /// Latest fact displayed to the user (e.g., "1.4 / 2.5 GB").
    pub detail: Option<String>,
    /// Error message set only when `status` is `Failed`.
    pub error: Option<String>,
    pub started_at_ms: u64,
    pub ended_at_ms: Option<u64>,
}
