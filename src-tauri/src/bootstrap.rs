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
use crate::transport;
use crate::transport::types::PortTransport;
use crate::workspace;
use crate::workspace::types::WorkspaceRegistry;

/// The process layer and the two subsystems built directly on it, constructed before the Tauri
/// builder so `backends::set_podman_runner` can run ahead of every `manage` call.
pub struct Bootstrap {
    mock_director: Arc<mockctl::types::MockDirector>,
    podman_runner: Arc<dyn ProcessRunner>,
    msb_runner: Arc<dyn ProcessRunner>,
    port_transport: Arc<dyn PortTransport>,
    machine: Arc<dyn machine::types::MachineManager>,
}

/// Builds the process layer and arms the podman global runner. Must be called before any
/// free-function podman call, which the `PODMAN_RUNNER` `OnceLock` in
/// `src-tauri/src/backends/mod.rs` otherwise panics on.
pub fn build() -> Bootstrap {
    let mock_director = Arc::new(mockctl::types::MockDirector::new());
    let podman_runner: Arc<dyn ProcessRunner> = if backends::mock_backend_enabled() {
        Arc::new(mockctl::types::ScriptedPodmanRunner::new(mock_director.clone()))
    } else {
        Arc::new(process::types::SystemPodmanRunner)
    };
    backends::set_podman_runner(podman_runner.clone());
    let msb_runner: Arc<dyn ProcessRunner> = if backends::mock_backend_enabled() {
        Arc::new(mockctl::types::ScriptedMsbRunner::new(mock_director.clone()))
    } else {
        Arc::new(process::types::SystemMsbRunner)
    };
    let port_transport = transport::default_port_transport();
    let machine = machine::default_machine_manager(podman_runner.clone());
    Bootstrap { mock_director, podman_runner, msb_runner, port_transport, machine }
}

impl Bootstrap {
    /// Registers every managed state entry and opens the main window. Called from the Tauri `setup`
    /// closure; `cleared` is `process::clear_inheritable_handles()`'s count, logged here because the
    /// sanitiser runs before there is a subscriber to write to.
    pub fn register_state(
        self,
        app: &tauri::App,
        cleared: usize,
    ) -> Result<(), Box<dyn std::error::Error>> {
        app.manage(init_logging(app.handle()));
        tracing::info!(target: "process", cleared, "cleared inheritable handles at startup");
        app.manage(self.machine);
        app.manage(machine::MachineRunners {
            podman: self.podman_runner.clone(),
            msb: self.msb_runner.clone(),
        });
        app.manage(self.mock_director.clone());
        // One registry shared by every subsystem, so a sandbox resolves to the same provider
        // instance (and the same base-image build lock) wherever it is addressed from.
        let registry = provider::default_registry(self.podman_runner.clone(), self.msb_runner.clone());
        app.manage(registry.clone());
        app.manage(runtime::default_runtime(registry.clone(), self.port_transport.clone()));
        // Needs `app.handle()` to resolve `crate::paths::app_data_root` for per-sandbox SSH
        // identities, so `default_backend` is constructed here rather than in `build`, unlike the
        // other subsystems above (none of which need an `AppHandle`).
        app.manage(backends::default_backend(
            registry.clone(),
            self.podman_runner.clone(),
            app.handle().clone(),
        ));
        if !backends::mock_backend_enabled() {
            if let Ok(resource_dir) = app.path().resource_dir() {
                backends::set_podman_resource_dir(resource_dir.join("podman"));
            }
        }
        let mutagen_runner: Arc<dyn ProcessRunner> = if backends::mock_backend_enabled() {
            Arc::new(mockctl::types::ScriptedMutagenRunner::new(self.mock_director.clone()))
        } else {
            Arc::new(process::types::SystemMutagenRunner::new(app.handle().clone()))
        };
        app.manage(sync::default_sync_engine(mutagen_runner));
        app.manage(network::default_network_manager(
            app.handle().clone(),
            self.port_transport.clone(),
            registry.clone(),
        ));
        // After the three registrations it borrows from.
        app.manage(SandboxRouter::new(
            app.state::<Arc<dyn SandboxBackend>>().inner().clone(),
            app.state::<Arc<dyn SyncEngine>>().inner().clone(),
            registry.clone(),
        ));
        app.manage(WorkspaceRegistry::default());
        app.manage(AuthSessionMap::default());
        app.manage(ActiveServer::default());
        app.manage(TaskRegistry::default());
        app.manage(MachineSpecsCache::default());
        workspace::create_main_window(app.handle())?;
        if !backends::mock_backend_enabled() {
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let live = match handle.state::<Arc<dyn SandboxBackend>>().list().await {
                    Ok(sandboxes) => sandboxes.into_iter().map(|s| s.name).collect::<Vec<_>>(),
                    Err(_) => return,
                };
                handle.state::<Arc<dyn NetworkManager>>().sweep(&live).await;
            });
        }
        Ok(())
    }
}

/// Stops the active inference server and shuts the sync engine down on exit.
pub fn on_run_event(app_handle: &tauri::AppHandle, event: tauri::RunEvent) {
    if let tauri::RunEvent::Exit = event {
        let active = app_handle.state::<ActiveServer>();
        let app_for_stop = app_handle.clone();
        tauri::async_runtime::block_on(async move {
            if let Some(handle) = active.lock().await.take() {
                let engine = inference::engine_for(handle.engine, &app_for_stop);
                let _ = engine.stop(handle).await;
            }
        });
        let engine = app_handle.state::<Arc<dyn SyncEngine>>();
        tauri::async_runtime::block_on(engine.shutdown());
    }
}
