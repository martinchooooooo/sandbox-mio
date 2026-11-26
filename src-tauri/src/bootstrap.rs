//! Startup wiring: the process layer every subsystem is built on, the Tauri state registrations,
//! and the exit handler.

use std::sync::Arc;

use tauri::Manager;

use crate::activity::TaskRegistry;
use crate::agents::types::AuthSessionMap;
use crate::backends;
use crate::backends::types::SandboxBackend;
use crate::inference;
use crate::inference::types::{ActiveServer, MachineSpecsCache};
use crate::logging::init_logging;
use crate::machine;
use crate::mockctl;
use crate::network;
use crate::network::types::NetworkManager;
use crate::process;
use crate::process::types::ProcessRunner;
use crate::provider;
use crate::runtime;
use crate::services::routing::SandboxRouter;
use crate::sync;
use crate::sync::types::SyncEngine;
use crate::workspace;
use crate::workspace::types::WorkspaceRegistry;

/// The process layer and the two subsystems built directly on it, constructed before the Tauri builder.
pub struct Bootstrap {
    mock_director: Arc<mockctl::types::MockDirector>,
    podman_runner: Arc<dyn ProcessRunner>,
    machine: Arc<dyn machine::types::MachineManager>,
}

/// Builds the process layer.
pub fn build() -> Bootstrap {
    let mock_director = Arc::new(mockctl::types::MockDirector::new());
    let podman_runner: Arc<dyn ProcessRunner> = if crate::domain::mock_backend_enabled() {
        Arc::new(mockctl::types::ScriptedPodmanRunner::new(mock_director.clone()))
    } else {
        Arc::new(process::podman::SystemPodmanRunner)
    };
    let machine = machine::default_machine_manager(podman_runner.clone());
    Bootstrap { mock_director, podman_runner, machine }
}

impl Bootstrap {
    /// Registers every managed state entry and opens the main window. Called from the Tauri `setup`
    /// closure; `cleared` is `process::windows::clear_inheritable_handles()`'s count, logged here because the
    /// sanitiser runs before there is a subscriber to write to.
    pub fn register_state(
        self,
        app: &tauri::App,
        cleared: usize,
    ) -> Result<(), Box<dyn std::error::Error>> {
        app.manage(init_logging(app.handle()));
        tracing::info!(target: "process", cleared, "cleared inheritable handles at startup");
        let proxy_store = crate::proxy::store::ProxyStore::for_app(app.handle());
        app.manage(proxy_store.clone());
        app.manage(self.machine);
        app.manage(machine::MachineRunners { podman: self.podman_runner.clone() });
        app.manage(self.mock_director.clone());
        // One registry shared by every subsystem, so a sandbox resolves to the same provider
        // instance (and the same base-image build lock) wherever it is addressed from.
        let registry = provider::default_registry(self.podman_runner.clone());
        app.manage(registry.clone());
        app.manage(runtime::default_runtime(
            registry.clone(),
            Arc::new(crate::services::toolchain_secrets::AppToolchainSecrets::new(
                app.handle().clone(),
                proxy_store.clone(),
            )),
        ));
        // Needs `app.handle()` to resolve `crate::paths::app_data_root` for per-sandbox SSH
        // identities, so `default_backend` is constructed here rather than in `build`, unlike the
        // other subsystems above (none of which need an `AppHandle`).
        app.manage(backends::default_backend(
            registry.clone(),
            self.podman_runner.clone(),
            app.handle().clone(),
        ));
        if !crate::domain::mock_backend_enabled() {
            if let Ok(resource_dir) = app.path().resource_dir() {
                crate::process::podman::set_podman_resource_dir(resource_dir.join("podman"));
            }
        }
        let mutagen_runner: Arc<dyn ProcessRunner> = if crate::domain::mock_backend_enabled() {
            Arc::new(mockctl::types::ScriptedMutagenRunner::new(self.mock_director.clone()))
        } else {
            Arc::new(process::mutagen::SystemMutagenRunner::new(app.handle().clone()))
        };
        app.manage(sync::default_sync_engine(mutagen_runner));
        app.manage(network::default_network_manager(
            app.handle().clone(),
            registry.clone(),
        ));
        // After the three registrations it borrows from.
        app.manage(SandboxRouter::new(
            app.state::<Arc<dyn SandboxBackend>>().inner().clone(),
            app.state::<Arc<dyn SyncEngine>>().inner().clone(),
            registry.clone(),
        ));
        app.manage(crate::agent_host::default_agent_host(registry.clone()));
        app.manage(crate::terminal::default_terminal_host(registry.clone(), self.mock_director.clone()));
        app.manage(WorkspaceRegistry::default());
        app.manage(crate::proxy::flowlog::FlowWatchRegistry::default());
        app.manage(AuthSessionMap::default());
        app.manage(ActiveServer::default());
        app.manage(TaskRegistry::default());
        app.manage(MachineSpecsCache::default());
        // After the registry, which the monitor samples through.
        let monitor = Arc::new(crate::resources::monitor::ResourceMonitor::default());
        monitor.start(
            app.handle().clone(),
            registry.clone(),
            crate::services::sandbox::is_user_sandbox,
        );
        app.manage(monitor);
        workspace::create_main_window(app.handle(), crate::services::panes::close_all_apps_in_background)?;
        if !crate::domain::mock_backend_enabled() {
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let live = match handle.state::<Arc<dyn SandboxBackend>>().list().await {
                    Ok(sandboxes) => sandboxes.into_iter().map(|s| s.name).collect::<Vec<_>>(),
                    Err(_) => return,
                };
                let proxy_store = handle.state::<crate::proxy::store::ProxyStore>();
                crate::proxy::migrate::migrate_legacy_secrets(&handle, &proxy_store, &live);
                handle.state::<Arc<dyn NetworkManager>>().sweep(&live).await;
                crate::proxy::sidecar::sweep(
                    &proxy_store,
                    handle.state::<Arc<provider::ProviderRegistry>>().inner(),
                    &live,
                )
                .await;
            });
        }
        Ok(())
    }
}

/// Stops the active inference server, closes its forwards, ends every chat attach stream and
/// terminal session, and shuts the sync engine down on exit.
pub fn on_run_event(app_handle: &tauri::AppHandle, event: tauri::RunEvent) {
    if let tauri::RunEvent::Exit = event {
        // First, so no stats call is in flight while the rest of the process layer winds down.
        app_handle.state::<Arc<crate::resources::monitor::ResourceMonitor>>().stop();
        let agent_host = app_handle.state::<Arc<dyn crate::agent_host::types::AgentHost>>().inner().clone();
        tauri::async_runtime::block_on(agent_host.detach_all());
        // Blocks until every session's exec child is killed; see `SshTerminalHost::detach_all`.
        let terminal = app_handle.state::<Arc<dyn crate::terminal::types::TerminalHost>>().inner().clone();
        tauri::async_runtime::block_on(terminal.detach_all());
        let active = app_handle.state::<ActiveServer>();
        let network = app_handle.state::<Arc<dyn NetworkManager>>().inner().clone();
        let app_for_stop = app_handle.clone();
        tauri::async_runtime::block_on(async move {
            if let Some(handle) = active.lock().await.take() {
                let engine = inference::engine_for(handle.engine, &app_for_stop);
                let _ = engine.stop(handle).await;
            }
            // Before `daemon stop`, so no inference session resumes against a stopped sandbox on the
            // next daemon start (`docs/features/local-llm-reachability/02_dev.md`, Resolved Decision 9).
            crate::services::inference_reach::close_all(&network).await;
        });
        let engine = app_handle.state::<Arc<dyn SyncEngine>>();
        tauri::async_runtime::block_on(engine.shutdown());
    }
}
