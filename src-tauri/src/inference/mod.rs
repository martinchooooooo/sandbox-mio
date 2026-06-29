//! Host-side local-inference subsystem: hardware probe, model download, and the two
//! `InferenceEngine` implementations (bundled llama.cpp+Vulkan and Foundry Local). Registered in
//! `lib.rs`; the single active server is held in Tauri state as [`types::ActiveServer`].

pub mod foundry;
pub mod llama;
pub mod types;

use std::sync::Arc;

use sha2::{Digest, Sha256};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};
use tokio::io::AsyncWriteExt;

use types::{EngineKind, InferenceEngine, InferenceError, MachineSpecs, ModelDownloadEvent};

/// Construct the chosen engine. Both impls hold the `AppHandle` (llama to resolve its bundled
/// resource dir, Foundry to satisfy a uniform constructor).
pub fn engine_for(kind: EngineKind, app: &AppHandle) -> Arc<dyn InferenceEngine> {
    match kind {
        EngineKind::Llama => Arc::new(llama::LlamaServerEngine { app: app.clone() }),
        EngineKind::Foundry => Arc::new(foundry::FoundryLocalEngine { app: app.clone() }),
    }
}

/// Root of the shared model store, `<app_data>/models`. Created if absent.
pub fn models_root(app: &AppHandle) -> Result<std::path::PathBuf, InferenceError> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| InferenceError::Download(e.to_string()))?
        .join("models");
    std::fs::create_dir_all(&dir).map_err(|e| InferenceError::Download(e.to_string()))?;
    Ok(dir)
}

/// Per-model directory `<app_data>/models/<model_id>`. Created if absent.
pub fn model_dir(app: &AppHandle, model_id: &str) -> Result<std::path::PathBuf, InferenceError> {
    let dir = models_root(app)?.join(model_id);
    std::fs::create_dir_all(&dir).map_err(|e| InferenceError::Download(e.to_string()))?;
    Ok(dir)
}

/// Catalogue model_ids with a verified local GGUF (a `.gguf` file in their model directory).
pub fn downloaded(app: &AppHandle) -> Result<Vec<String>, InferenceError> {
    let root = models_root(app)?;
    let mut ids = Vec::new();
    for entry in std::fs::read_dir(&root).into_iter().flatten().flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let has_gguf = std::fs::read_dir(entry.path()).into_iter().flatten().flatten().any(|f| {
            f.path()
                .extension()
                .and_then(|x| x.to_str())
                .is_some_and(|x| x.eq_ignore_ascii_case("gguf"))
        });
        if has_gguf {
            if let Some(name) = entry.file_name().to_str() {
                ids.push(name.to_string());
            }
        }
    }
    Ok(ids)
}

/// Stream `https://huggingface.co/<repo>/resolve/main/<file>` into the model store, emitting
/// [`ModelDownloadEvent`] progress, then verify sha256 and atomically rename into place. A no-op
/// when the verified file already exists. The partial `.part` is deleted on any failure.
pub async fn download_gguf(
    app: &AppHandle,
    model_id: &str,
    repo: &str,
    file: &str,
    sha256: &str,
    on_progress: &Channel<serde_json::Value>,
) -> Result<(), InferenceError> {
    use futures_util::StreamExt;

    let dir = model_dir(app, model_id)?;
    let final_path = dir.join(file);
    if final_path.exists() {
        // Already downloaded; presence is the cheap integrity proxy (download verified its sha256).
        on_progress.send(serde_json::to_value(ModelDownloadEvent::Done).unwrap()).ok();
        return Ok(());
    }
    let part_path = dir.join(format!("{file}.part"));

    let url = format!("https://huggingface.co/{repo}/resolve/main/{file}");
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| InferenceError::Download(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(InferenceError::Download(format!("HTTP {} from {url}", resp.status())));
    }
    let total = resp.content_length().unwrap_or(0);
    on_progress
        .send(serde_json::to_value(ModelDownloadEvent::Started { total_bytes: total }).unwrap())
        .ok();

    let mut file_out = tokio::fs::File::create(&part_path)
        .await
        .map_err(|e| InferenceError::Download(e.to_string()))?;
    // Drives the global activity center in addition to the per-invoke `Channel` (which stays for
    // the in-context `/models` progress bar). The id matches the one `download_model` opened.
    let task_id = format!("model-download:{model_id}");
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let mut hasher = Sha256::new();
    let mut downloaded: u64 = 0;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| InferenceError::Download(e.to_string()))?;
        hasher.update(&chunk);
        file_out
            .write_all(&chunk)
            .await
            .map_err(|e| InferenceError::Download(e.to_string()))?;
        downloaded += chunk.len() as u64;
        on_progress
            .send(
                serde_json::to_value(ModelDownloadEvent::Progress {
                    downloaded_bytes: downloaded,
                    total_bytes: total,
                })
                .unwrap(),
            )
            .ok();
        let fraction = (total > 0).then(|| downloaded as f32 / total as f32);
        let detail = if total > 0 {
            format!("{:.1} / {:.1} GB", downloaded as f64 / GB, total as f64 / GB)
        } else {
            format!("{:.1} GB", downloaded as f64 / GB)
        };
        crate::activity::progress(app, &task_id, fraction, Some(detail));
    }
    file_out.flush().await.map_err(|e| InferenceError::Download(e.to_string()))?;
    drop(file_out);

    on_progress.send(serde_json::to_value(ModelDownloadEvent::Verifying).unwrap()).ok();
    let actual = format!("{:x}", hasher.finalize());
    // An empty/placeholder catalogue sha256 means "not yet pinned"; skip the check rather than
    // fail the first bring-up download that establishes the real hash.
    if !sha256.is_empty() && !actual.eq_ignore_ascii_case(sha256) {
        let _ = tokio::fs::remove_file(&part_path).await;
        return Err(InferenceError::ChecksumMismatch {
            expected: sha256.to_string(),
            actual,
        });
    }
    tokio::fs::rename(&part_path, &final_path)
        .await
        .map_err(|e| InferenceError::Download(e.to_string()))?;
    on_progress.send(serde_json::to_value(ModelDownloadEvent::Done).unwrap()).ok();
    Ok(())
}

/// Probe host CPU cores + RAM (`sysinfo`, cross-platform) and, on Windows, the GPU/NPU and
/// Foundry availability. Run on the host, never the container VM (whose 2 GiB cap is misleading).
pub fn detect_specs(_app: &AppHandle) -> MachineSpecs {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_memory();
    sys.refresh_cpu_all();
    let cpu_cores = sys.physical_core_count().unwrap_or_else(|| sys.cpus().len());

    MachineSpecs {
        cpu_cores,
        total_ram_bytes: sys.total_memory(),
        free_ram_bytes: sys.available_memory(),
        gpu_names: gpu_names(),
        has_npu: has_npu(),
        arch: std::env::consts::ARCH.to_string(),
        foundry_installed: foundry_installed(),
        foundry_installable: foundry_installable(),
    }
}

#[cfg(windows)]
fn powershell(script: &str) -> Option<String> {
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(windows)]
fn gpu_names() -> Vec<String> {
    powershell("Get-CimInstance Win32_VideoController | Select-Object -ExpandProperty Name")
        .map(|s| s.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default()
}

#[cfg(windows)]
fn has_npu() -> bool {
    // A Copilot+-class NPU exposes a PnP device whose name contains "AI Boost" / "NPU".
    powershell(
        "(Get-PnpDevice -Status OK -ErrorAction SilentlyContinue | \
         Where-Object { $_.FriendlyName -match 'AI Boost|Neural|NPU' } | Measure-Object).Count",
    )
    .and_then(|s| s.trim().parse::<u32>().ok())
    .is_some_and(|n| n > 0)
}

#[cfg(windows)]
fn foundry_installed() -> bool {
    std::process::Command::new("foundry")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(windows)]
fn foundry_installable() -> bool {
    std::process::Command::new("winget")
        .args(["show", "Microsoft.FoundryLocal"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(not(windows))]
fn gpu_names() -> Vec<String> {
    Vec::new()
}
#[cfg(not(windows))]
fn has_npu() -> bool {
    false
}
#[cfg(not(windows))]
fn foundry_installed() -> bool {
    false
}
#[cfg(not(windows))]
fn foundry_installable() -> bool {
    false
}

/// Install Foundry Local via `winget install Microsoft.FoundryLocal`, streaming raw CLI output
/// lines to `on_progress` as `{ kind: "Output", line }` so the advanced-settings UI can show it.
#[cfg(windows)]
pub async fn install_foundry(on_progress: &Channel<serde_json::Value>) -> Result<(), InferenceError> {
    use tokio::io::AsyncBufReadExt;

    let mut child = tokio::process::Command::new("winget")
        .args([
            "install",
            "Microsoft.FoundryLocal",
            "--accept-package-agreements",
            "--accept-source-agreements",
            "--disable-interactivity",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| InferenceError::CommandFailed(e.to_string()))?;

    if let Some(stdout) = child.stdout.take() {
        let mut lines = tokio::io::BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            on_progress
                .send(serde_json::json!({ "kind": "Output", "line": line }))
                .ok();
        }
    }
    let status = child.wait().await.map_err(|e| InferenceError::CommandFailed(e.to_string()))?;
    if !status.success() {
        return Err(InferenceError::CommandFailed("winget install failed".into()));
    }
    Ok(())
}

#[cfg(not(windows))]
pub async fn install_foundry(_on_progress: &Channel<serde_json::Value>) -> Result<(), InferenceError> {
    Err(InferenceError::FoundryUnavailable)
}
