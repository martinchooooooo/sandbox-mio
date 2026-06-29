//! Foundry Local engine. Drives the first-party `foundry` CLI (installed via winget
//! `Microsoft.FoundryLocal`) which serves an OpenAI-compatible endpoint on a dynamic port and
//! auto-selects an OpenVINO execution-provider variant per device (NPU > GPU > CPU).
//!
//! Verified command surface (spike 2026-06-29): `foundry service start|status`,
//! `foundry model download|load|unload <alias>`. The non-interactive load path is
//! `download` then `load` (not `model run`, which opens an interactive REPL).

use std::time::Duration;

use tauri::ipc::Channel;
use tauri::AppHandle;

use super::types::{
    EngineKind, InferenceEngine, InferenceError, RuntimeSetupEvent, ServerHandle, SetupStep,
};

pub struct FoundryLocalEngine {
    #[allow(dead_code)]
    pub app: AppHandle,
}

/// Run `foundry <args>` and return trimmed stdout, mapping a missing binary to
/// [`InferenceError::FoundryUnavailable`] and a non-zero exit to [`InferenceError::CommandFailed`].
///
/// Every invocation is traced (argv, exit status, elapsed, stdout/stderr) so a wedged or failing
/// subprocess is visible in the devtools console — the inference module is otherwise opaque.
async fn foundry(args: &[&str]) -> Result<String, InferenceError> {
    let argv = args.join(" ");
    let started = std::time::Instant::now();
    tracing::info!(target: "inference::foundry", cmd = %format!("foundry {argv}"), "spawning");

    let out = tokio::process::Command::new("foundry")
        .args(args)
        .output()
        .await
        .map_err(|e| {
            tracing::error!(target: "inference::foundry", cmd = %format!("foundry {argv}"), error = %e, "spawn failed — binary missing?");
            InferenceError::FoundryUnavailable
        })?;

    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    let elapsed_ms = started.elapsed().as_millis();
    if out.status.success() {
        tracing::info!(
            target: "inference::foundry",
            cmd = %format!("foundry {argv}"), elapsed_ms, %stdout,
            "ok"
        );
        Ok(stdout)
    } else {
        tracing::error!(
            target: "inference::foundry",
            cmd = %format!("foundry {argv}"), elapsed_ms, code = ?out.status.code(), %stderr, %stdout,
            "non-zero exit"
        );
        Err(InferenceError::CommandFailed(stderr))
    }
}

/// Run a foundry command for a step that can take a long time (download/load), emitting a
/// [`RuntimeSetupEvent::StepProgress`] heartbeat with the elapsed seconds every few seconds while
/// it runs. This turns an opaque spinner into observable liveness so a slow step is distinct from
/// a hang. The command's own output is still captured by [`foundry`] tracing.
async fn foundry_heartbeat(
    args: &[&str],
    step_id: &str,
    on_step: &Channel<serde_json::Value>,
) -> Result<String, InferenceError> {
    let started = std::time::Instant::now();
    let run = foundry(args);
    tokio::pin!(run);
    let mut ticker = tokio::time::interval(Duration::from_secs(3));
    ticker.tick().await; // consume the immediate first tick
    loop {
        tokio::select! {
            result = &mut run => return result,
            _ = ticker.tick() => {
                RuntimeSetupEvent::StepProgress {
                    id: step_id.to_string(),
                    detail: format!("{}s", started.elapsed().as_secs()),
                }
                .send(on_step);
            }
        }
    }
}

/// Extract the first `http://127.0.0.1:<port>` URL from CLI output (`service start`/`status`).
fn parse_endpoint(text: &str) -> Option<(String, u16)> {
    let marker = "127.0.0.1:";
    let idx = text.find(marker)?;
    let after = &text[idx + marker.len()..];
    let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
    let port: u16 = digits.parse().ok()?;
    Some((format!("http://127.0.0.1:{port}/v1"), port))
}

#[async_trait::async_trait]
impl InferenceEngine for FoundryLocalEngine {
    fn kind(&self) -> EngineKind {
        EngineKind::Foundry
    }

    fn setup_steps(&self) -> Vec<SetupStep> {
        ["check-foundry", "register-eps", "start-service", "model-ready", "load-model", "resolve-id"]
            .iter()
            .map(|id| SetupStep::new(id))
            .collect()
    }

    async fn start(
        &self,
        model_id: &str,
        _port: u16,
        on_step: &Channel<serde_json::Value>,
    ) -> Result<ServerHandle, InferenceError> {
        let alias = model_id;

        // check-foundry: the CLI must be installed (it is a system component, not a sidecar).
        RuntimeSetupEvent::StepStart { id: "check-foundry".into() }.send(on_step);
        match foundry(&["--version"]).await {
            Ok(v) => RuntimeSetupEvent::StepDone {
                id: "check-foundry".into(),
                detail: Some(v.lines().next().unwrap_or("").to_string()),
            }
            .send(on_step),
            Err(e) => {
                RuntimeSetupEvent::StepFailed { id: "check-foundry".into(), error: e.to_string() }
                    .send(on_step);
                return Err(InferenceError::FoundryUnavailable);
            }
        }

        // register-eps: idempotent execution-provider registration.
        RuntimeSetupEvent::StepStart { id: "register-eps".into() }.send(on_step);
        let _ = foundry(&["service", "init"]).await; // best-effort; absent on some CLI versions
        RuntimeSetupEvent::StepDone { id: "register-eps".into(), detail: None }.send(on_step);

        // start-service: idempotent; parse the dynamic endpoint from start or status output.
        RuntimeSetupEvent::StepStart { id: "start-service".into() }.send(on_step);
        let start_out = foundry(&["service", "start"]).await.unwrap_or_default();
        let endpoint = match parse_endpoint(&start_out) {
            Some(e) => Some(e),
            None => foundry(&["service", "status"]).await.ok().and_then(|s| parse_endpoint(&s)),
        };
        let (base_url, port) = match endpoint {
            Some(e) => e,
            None => {
                let err = InferenceError::ServerNotReady("could not parse Foundry endpoint".into());
                RuntimeSetupEvent::StepFailed { id: "start-service".into(), error: err.to_string() }
                    .send(on_step);
                return Err(err);
            }
        };
        RuntimeSetupEvent::StepDone {
            id: "start-service".into(),
            detail: Some(format!("port {port}")),
        }
        .send(on_step);

        // model-ready: download the hardware-matched variant (no-op when already cached).
        RuntimeSetupEvent::StepStart { id: "model-ready".into() }.send(on_step);
        foundry_heartbeat(&["model", "download", alias], "model-ready", on_step).await.map_err(|e| {
            RuntimeSetupEvent::StepFailed { id: "model-ready".into(), error: e.to_string() }
                .send(on_step);
            e
        })?;
        RuntimeSetupEvent::StepDone { id: "model-ready".into(), detail: None }.send(on_step);

        // load-model: load into the running service.
        RuntimeSetupEvent::StepStart { id: "load-model".into() }.send(on_step);
        foundry_heartbeat(&["model", "load", alias], "load-model", on_step).await.map_err(|e| {
            RuntimeSetupEvent::StepFailed { id: "load-model".into(), error: e.to_string() }
                .send(on_step);
            e
        })?;
        RuntimeSetupEvent::StepDone { id: "load-model".into(), detail: None }.send(on_step);

        // resolve-id: read back the device-specific served id; requests must use it, not the alias.
        RuntimeSetupEvent::StepStart { id: "resolve-id".into() }.send(on_step);
        let served_id = resolve_served_id(&base_url).await.unwrap_or_else(|| alias.to_string());
        RuntimeSetupEvent::StepDone {
            id: "resolve-id".into(),
            detail: Some(served_id.clone()),
        }
        .send(on_step);

        RuntimeSetupEvent::Ready { base_url: base_url.clone(), model_id: served_id.clone() }
            .send(on_step);

        Ok(ServerHandle {
            engine: EngineKind::Foundry,
            model_id: served_id,
            alias: alias.to_string(),
            port,
            base_url,
            child: None,
        })
    }

    async fn stop(&self, handle: ServerHandle) -> Result<(), InferenceError> {
        // Leave the OS-managed service running; only unload the model so it stops holding RAM.
        let _ = foundry(&["model", "unload", &handle.alias]).await;
        Ok(())
    }
}

/// `GET <base_url>/models` and return `data[0].id`, the device-specific model id Foundry serves
/// (e.g. `qwen2.5-0.5b-instruct-openvino-npu:5`) — sending the catalogue alias returns HTTP 400.
async fn resolve_served_id(base_url: &str) -> Option<String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{base_url}/models"))
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .ok()?;
    let json: serde_json::Value = resp.json().await.ok()?;
    json.get("data")?
        .as_array()?
        .first()?
        .get("id")?
        .as_str()
        .map(|s| s.to_string())
}
