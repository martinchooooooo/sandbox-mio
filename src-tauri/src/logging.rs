//! File logging setup. Kept apart from `bootstrap.rs` because it is mutually exclusive with
//! `tauri-plugin-devtools`, which claims the global tracing subscriber for itself.

use tauri::Manager;
use tracing_subscriber::prelude::*;

use crate::env_ext;

/// Returns the `WorkerGuard` for the caller to `app.manage()`. Its `Drop` is the only code path
/// that signals the non-blocking writer's worker thread to shut down and blocks briefly for it to
/// drain — dropping it silently (or leaking it, as this used to) loses whatever was queued but not
/// yet written when the process exits.
pub fn init_logging(app: &tauri::AppHandle) -> tracing_appender::non_blocking::WorkerGuard {
    // Unset must resolve exactly as before: app_log_dir() is Local AppData, not a subdirectory of
    // app_data_dir() (Roaming), so folding the unset case into the override would relocate every
    // user's log (docs/features/parallel-worktree-e2e/02_dev.md, Resolved Decision 9).
    let log_dir = match env_ext::env_dir("MIO_APP_DATA_DIR") {
        Some(root) => root.join("logs"),
        None => app.path().app_log_dir().unwrap_or_else(|_| std::env::temp_dir()),
    };
    let _ = std::fs::create_dir_all(&log_dir);

    let file_appender = tracing_appender::rolling::daily(&log_dir, "mio.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    // A failed `try_init` means something else (in practice `tauri-plugin-devtools`, see `run()`)
    // already owns the global subscriber, so every `tracing!` call bypasses the file below and
    // can't report the failure itself — write it straight to the file instead.
    if let Err(e) = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_ansi(false).with_writer(non_blocking))
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stdout))
        .try_init()
    {
        if let Ok(mut f) =
            std::fs::OpenOptions::new().create(true).append(true).open(log_dir.join("mio.log"))
        {
            use std::io::Write;
            let _ = writeln!(
                f,
                "logging not initialized: another global tracing subscriber is already installed \
                 ({e}); no events will reach this file for this run"
            );
        }
    }

    let panic_log_path = log_dir.join("mio-panic.log");
    std::panic::set_hook(Box::new(move |info| {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&panic_log_path) {
            use std::io::Write;
            let _ = writeln!(f, "{:?}: {info}", std::time::SystemTime::now());
        }
        tracing::error!("panic: {info}");
    }));

    tracing::info!(log_dir = %log_dir.display(), "logging initialized");
    guard
}
