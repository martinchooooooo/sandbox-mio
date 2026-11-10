//! Keeps one `inference-<sandbox>` reverse forward per running local sandbox while a server runs, so
//! agents reach it on the sandbox's own loopback. Best-effort throughout: failures are logged, never
//! raised (`docs/features/local-llm-reachability/02_dev.md`, Resolved Decision 10).

use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::backends::types::{Sandbox, SandboxBackend};
use crate::inference::types::ActiveServer;
use crate::inference::INFERENCE_GUEST_PORT;
use crate::network::types::{NetworkManager, ServiceKind};
use crate::provider::ProviderRegistry;
use crate::services::routing::SandboxRouter;

/// The running server's host port, read under the `ActiveServer` lock and released before any
/// Mutagen call.
async fn server_port(app: &AppHandle) -> Option<u16> {
    let active = app.state::<ActiveServer>().inner().clone();
    let guard = active.lock().await;
    guard.as_ref().map(|h| h.port)
}

/// Ensures `name`'s inference forward if a server runs and the sandbox is running locally.
pub async fn ensure_for_sandbox(app: &AppHandle, name: &str) {
    let Some(port) = server_port(app).await else { return };
    let store = app.state::<Arc<dyn SandboxBackend>>().inner().clone();
    let sandboxes = match super::sandbox::list_user(&store).await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(target: "inference", sandbox = %name, error = %e, "could not list sandboxes for the inference forward");
            return;
        }
    };
    // Dialling a stopped microsandbox sandbox's alias tries to boot its VM without the app's start
    // environment (`docs/features/local-llm-reachability/spike/01_mutagen-reverse-msb.md`).
    if !sandboxes.iter().any(|s| s.name == name && is_running(s)) {
        return;
    }
    ensure_with_port(app, name, port).await;
}

/// Ensures the inference forward of every running local sandbox; recreates forwards that target
/// an earlier server's port.
pub async fn ensure_all(app: &AppHandle) {
    let Some(port) = server_port(app).await else { return };
    let store = app.state::<Arc<dyn SandboxBackend>>().inner().clone();
    let sandboxes = match super::sandbox::list_user(&store).await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(target: "inference", error = %e, "could not list sandboxes for inference forwards");
            return;
        }
    };
    for sandbox in sandboxes.iter().filter(|s| is_running(s)) {
        ensure_with_port(app, &sandbox.name, port).await;
    }
}

/// [`ensure_for_sandbox`] off the caller's path, so a sandbox or server command never waits on
/// forward creation.
pub fn spawn_ensure_for_sandbox(app: &AppHandle, name: &str) {
    let app = app.clone();
    let name = name.to_string();
    tauri::async_runtime::spawn(async move { ensure_for_sandbox(&app, &name).await });
}

/// [`ensure_all`] off the caller's path.
pub fn spawn_ensure_all(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move { ensure_all(&app).await });
}

/// Closes every tracked inference forward.
pub async fn close_all(network: &Arc<dyn NetworkManager>) {
    let sandboxes: Vec<String> = network
        .list_forwards()
        .await
        .into_iter()
        .filter(|f| f.kind == ServiceKind::Inference)
        .map(|f| f.sandbox)
        .collect();
    for sandbox in sandboxes {
        network.close_forward(&sandbox, ServiceKind::Inference).await;
    }
}

/// Closes `name`'s inference forward, if any.
pub async fn close_for_sandbox(network: &Arc<dyn NetworkManager>, name: &str) {
    network.close_forward(name, ServiceKind::Inference).await;
}

fn is_running(sandbox: &Sandbox) -> bool {
    sandbox.status == "running"
}

async fn ensure_with_port(app: &AppHandle, name: &str, port: u16) {
    let router = app.state::<SandboxRouter>();
    let route = match router.resolve(name, app).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(target: "inference", sandbox = %name, error = %e, "could not resolve the sandbox for its inference forward");
            return;
        }
    };
    // Deployed sandboxes are out of scope (Resolved Decision 3).
    let registry = app.state::<Arc<ProviderRegistry>>();
    if route.machine != registry.local_ref(route.machine.provider) {
        return;
    }
    let network = app.state::<Arc<dyn NetworkManager>>().inner().clone();
    let current = network
        .list_forwards()
        .await
        .into_iter()
        .any(|f| f.sandbox == name && f.kind == ServiceKind::Inference && f.host_port == port);
    if current {
        return;
    }
    match network
        .open_reverse_forward(
            name,
            ServiceKind::Inference,
            &route.forward_alias,
            INFERENCE_GUEST_PORT,
            port,
        )
        .await
    {
        Ok(()) => tracing::info!(target: "inference", sandbox = %name, host_port = port, "inference forward ready"),
        Err(e) => tracing::warn!(target: "inference", sandbox = %name, error = %e, "inference forward failed"),
    }
}
