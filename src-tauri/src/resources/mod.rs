//! The resource monitor (`docs/features/sandbox-resource-monitor/02_dev.md`): one background task
//! samples every running local sandbox's CPU and memory each tick, pushes the live snapshot to the
//! frontend as `resources://snapshot`, and appends one-minute rollups to a JSON-lines history file
//! per sandbox under `paths::resource_history_dir`.
//!
//! `monitor` owns the task and the live snapshot, `rollup` folds samples into minute rollups, and
//! `history` reads, appends and prunes the history files.

pub mod history;
pub mod monitor;
pub mod rollup;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn unix_now() -> Duration {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default()
}
