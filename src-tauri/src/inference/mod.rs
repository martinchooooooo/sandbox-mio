//! Host-side local-inference: hardware probe, model download, and engine implementations.

pub mod foundry;
pub mod llama;
pub mod types;

use std::sync::Arc;

use sha2::{Digest, Sha256};
use tauri::ipc::Channel;
use tauri::AppHandle;
use tokio::io::AsyncWriteExt;

use crate::process::{detached_command, detached_command_std};
use types::{
    EngineKind, InferenceEngine, InferenceError, MachineSpecs, MachineSpecsCache,
    ModelDownloadEvent, StaticMachineSpecs,
};

/// The Mutagen forward carrying the inference server into the Podman VM. Named here because both
/// `start_inference` and `stop_inference` address it.
pub const INFERENCE_FORWARD: &str = "inference-reverse";

/// A free host loopback port for a server about to start. Bound and released, so the caller races
/// anything else on the host for it — acceptable because the engine binds it immediately after.
pub fn ephemeral_port() -> Result<u16, InferenceError> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| InferenceError::Sidecar(e.to_string()))?;
    let port = listener
        .local_addr()
        .map_err(|e| InferenceError::Sidecar(e.to_string()))?
        .port();
    Ok(port)
}

pub fn engine_for(kind: EngineKind, app: &AppHandle) -> Arc<dyn InferenceEngine> {
    match kind {
        EngineKind::Llama => Arc::new(llama::LlamaServerEngine { app: app.clone() }),
        EngineKind::Foundry => Arc::new(foundry::FoundryLocalEngine { app: app.clone() }),
    }
}

/// `MIO_APP_DATA_DIR` relocates the model store (see [`crate::paths::app_data_root`]); no
/// subsystem-specific override exists for this one.
pub fn models_root(app: &AppHandle) -> Result<std::path::PathBuf, InferenceError> {
    let dir = crate::paths::app_data_root(app)
        .map_err(|e| InferenceError::Download(e.to_string()))?
        .join("models");
    std::fs::create_dir_all(&dir).map_err(|e| InferenceError::Download(e.to_string()))?;
    Ok(dir)
}

pub fn model_dir(app: &AppHandle, model_id: &str) -> Result<std::path::PathBuf, InferenceError> {
    let dir = models_root(app)?.join(model_id);
    std::fs::create_dir_all(&dir).map_err(|e| InferenceError::Download(e.to_string()))?;
    Ok(dir)
}

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
    let task_id = format!("model-download:{model_id}");
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    const PROGRESS_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);
    let mut hasher = Sha256::new();
    let mut downloaded: u64 = 0;
    let mut last_emit = std::time::Instant::now();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| InferenceError::Download(e.to_string()))?;
        hasher.update(&chunk);
        file_out
            .write_all(&chunk)
            .await
            .map_err(|e| InferenceError::Download(e.to_string()))?;
        downloaded += chunk.len() as u64;
        if last_emit.elapsed() < PROGRESS_INTERVAL {
            continue;
        }
        last_emit = std::time::Instant::now();
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
    on_progress
        .send(
            serde_json::to_value(ModelDownloadEvent::Progress { downloaded_bytes: downloaded, total_bytes: total })
                .unwrap(),
        )
        .ok();

    on_progress.send(serde_json::to_value(ModelDownloadEvent::Verifying).unwrap()).ok();
    let actual = format!("{:x}", hasher.finalize());
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

fn probe_static() -> StaticMachineSpecs {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_cpu_all();
    let cpu_cores = sys.physical_core_count().unwrap_or_else(|| sys.cpus().len());

    StaticMachineSpecs {
        cpu_cores,
        gpu_names: gpu_names(),
        has_npu: has_npu(),
        arch: std::env::consts::ARCH.to_string(),
        foundry_installed: foundry_installed(),
        foundry_installable: foundry_installable(),
    }
}

pub fn detect_specs(cache: &MachineSpecsCache) -> MachineSpecs {
    let static_specs = {
        let mut guard = cache.lock().unwrap();
        match guard.as_ref() {
            Some(s) => s.clone(),
            None => {
                let s = probe_static();
                *guard = Some(s.clone());
                s
            }
        }
    };

    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_memory();

    MachineSpecs {
        cpu_cores: static_specs.cpu_cores,
        total_ram_bytes: sys.total_memory(),
        free_ram_bytes: sys.available_memory(),
        gpu_names: static_specs.gpu_names,
        has_npu: static_specs.has_npu,
        arch: static_specs.arch,
        foundry_installed: static_specs.foundry_installed,
        foundry_installable: static_specs.foundry_installable,
    }
}

pub fn redetect_specs(cache: &MachineSpecsCache) -> MachineSpecs {
    *cache.lock().unwrap() = None;
    detect_specs(cache)
}

#[cfg(windows)]
fn powershell(script: &str) -> Option<String> {
    let out = detached_command_std("powershell")
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
    powershell(
        "(Get-PnpDevice -Status OK -ErrorAction SilentlyContinue | \
         Where-Object { $_.FriendlyName -match 'AI Boost|Neural|NPU' } | Measure-Object).Count",
    )
    .and_then(|s| s.trim().parse::<u32>().ok())
    .is_some_and(|n| n > 0)
}

#[cfg(windows)]
fn foundry_installed() -> bool {
    detached_command_std("foundry")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(windows)]
fn foundry_installable() -> bool {
    detached_command_std("winget")
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

#[cfg(windows)]
pub async fn install_foundry(on_progress: &Channel<serde_json::Value>) -> Result<(), InferenceError> {
    use tokio::io::AsyncBufReadExt;

    let mut child = detached_command("winget")
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
