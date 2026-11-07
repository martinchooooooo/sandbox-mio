//! In-memory mock of [`NetworkManager`], gated by `MIO_MOCK_BACKEND=1` (see
//! `default_network_manager`). Lets the e2e "light" test tier drive real Tauri commands without
//! spawning the Mutagen sidecar or shelling out to `podman`.

use std::collections::HashMap;
use std::sync::Mutex;

use tauri::{AppHandle, Emitter};

use super::types::{ForwardInfo, NetworkError, NetworkManager, ServiceKind, EVENT_NAME};
use super::{allocate_ephemeral_port, ensure_ports_file};

/// Fabricates plausible host ports and records them in-memory; never touches podman or Mutagen.
/// The light tier never opens a terminal/agent-web pane or exposes a port today, but this keeps
/// the mock behaviorally consistent (list/close round-trip correctly, `EVENT_NAME` emitted on every
/// mutation) if that changes.
pub(super) struct MockNetworkManager {
    app: AppHandle,
    forwards: Mutex<HashMap<(String, ServiceKind), ForwardInfo>>,
    ports_watch: Mutex<HashMap<String, tokio::task::JoinHandle<()>>>,
}

impl MockNetworkManager {
    pub(super) fn new(app: AppHandle) -> Self {
        Self {
            app,
            forwards: Mutex::new(HashMap::new()),
            ports_watch: Mutex::new(HashMap::new()),
        }
    }

    fn emit_forwards(&self) {
        let snapshot: Vec<ForwardInfo> = self.forwards.lock().unwrap().values().cloned().collect();
        let _ = self.app.emit(EVENT_NAME, &snapshot);
    }
}

#[async_trait::async_trait]
impl NetworkManager for MockNetworkManager {
    async fn open_forward(
        &self,
        sandbox: &str,
        kind: ServiceKind,
        alias: &str,
        remote_port: u16,
    ) -> Result<u16, NetworkError> {
        let host_port = allocate_ephemeral_port()?;
        self.forwards.lock().unwrap().insert(
            (sandbox.to_string(), kind),
            ForwardInfo {
                sandbox: sandbox.to_string(),
                kind,
                host_port,
                connection: alias.to_string(),
                remote_port,
                forward_name: format!("mock-{sandbox}"),
                label: None,
                auto_detected: false,
            },
        );
        self.emit_forwards();
        Ok(host_port)
    }

    async fn close_forward(&self, sandbox: &str, kind: ServiceKind) {
        self.forwards.lock().unwrap().remove(&(sandbox.to_string(), kind));
        self.emit_forwards();
    }

    async fn close_sandbox(&self, sandbox: &str) {
        if let Some(handle) = self.ports_watch.lock().unwrap().remove(sandbox) {
            handle.abort();
        }
        self.forwards.lock().unwrap().retain(|(s, _), _| s != sandbox);
        self.emit_forwards();
    }

    async fn list_forwards(&self) -> Vec<ForwardInfo> {
        self.forwards.lock().unwrap().values().cloned().collect()
    }

    async fn sweep(&self, _live_sandboxes: &[String]) {}

    async fn expose_port(
        &self,
        sandbox: &str,
        _machine: &crate::provider::types::MachineRef,
        workspace_path: &str,
        target_port: u16,
        label: Option<String>,
        auto_detected: bool,
    ) -> Result<u16, NetworkError> {
        let kind = ServiceKind::Exposed(target_port);
        let host_port = allocate_ephemeral_port()?;
        self.forwards.lock().unwrap().insert(
            (sandbox.to_string(), kind),
            ForwardInfo {
                sandbox: sandbox.to_string(),
                kind,
                host_port,
                connection: sandbox.to_string(),
                remote_port: target_port,
                forward_name: format!("mock-expose-{sandbox}-{target_port}"),
                label: label.clone(),
                auto_detected,
            },
        );
        self.emit_forwards();
        ensure_ports_file(workspace_path);
        Ok(host_port)
    }

    async fn unexpose_port(&self, sandbox: &str, target_port: u16) {
        self.forwards
            .lock()
            .unwrap()
            .remove(&(sandbox.to_string(), ServiceKind::Exposed(target_port)));
        self.emit_forwards();
    }

    async fn watch_ports_file(
        self: std::sync::Arc<Self>,
        sandbox: &str,
        workspace_path: &str,
        machine: &crate::provider::types::MachineRef,
    ) {
        let mut watches = self.ports_watch.lock().unwrap();
        if watches.contains_key(sandbox) {
            return;
        }
        let path = std::path::Path::new(workspace_path).join(".mio").join("ports.json");
        let sandbox = sandbox.to_string();
        let workspace_path = workspace_path.to_string();
        let machine = machine.clone();
        let manager = self.clone();
        let task_sandbox = sandbox.clone();
        let handle = tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                let Ok(contents) = tokio::fs::read_to_string(&path).await else { continue };
                let Ok(raw) = serde_json::from_str::<HashMap<String, String>>(&contents) else { continue };
                let declared: HashMap<u16, String> = raw
                    .into_iter()
                    .filter_map(|(port, label)| port.parse::<u16>().ok().map(|p| (p, label)))
                    .collect();

                let (file_managed, already_exposed): (Vec<u16>, std::collections::HashSet<u16>) = {
                    let forwards = manager.forwards.lock().unwrap();
                    let exposed_for_sandbox: Vec<&ForwardInfo> = forwards
                        .values()
                        .filter(|f| f.sandbox == task_sandbox)
                        .collect();
                    let file_managed = exposed_for_sandbox
                        .iter()
                        .filter(|f| f.label.is_some())
                        .filter_map(|f| match f.kind {
                            ServiceKind::Exposed(p) => Some(p),
                            _ => None,
                        })
                        .collect();
                    let already_exposed = exposed_for_sandbox
                        .iter()
                        .filter_map(|f| match f.kind {
                            ServiceKind::Exposed(p) => Some(p),
                            _ => None,
                        })
                        .collect();
                    (file_managed, already_exposed)
                };

                for port in file_managed.into_iter().filter(|p| !declared.contains_key(p)) {
                    manager.unexpose_port(&task_sandbox, port).await;
                }

                for (port, label) in declared.into_iter().filter(|(p, _)| !already_exposed.contains(p)) {
                    let _ = manager
                        .expose_port(&task_sandbox, &machine, &workspace_path, port, Some(label), false)
                        .await;
                }
            }
        });
        watches.insert(sandbox, handle);
    }
}
