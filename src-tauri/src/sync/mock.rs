//! In-memory mock of [`SyncEngine`], gated by `MIO_MOCK_BACKEND=1` (see `default_sync_engine`).
//! Lets the e2e "light" test tier drive real Tauri commands without spawning the Mutagen sidecar.

use super::types::{SessionInfo, SyncEngine, SyncError, SyncStatus};

/// Stateless stand-in: every session lookup reports "already synced" and there is nothing to
/// list, since the light tier never deploys a sandbox.
pub(super) struct MockSync;

#[async_trait::async_trait]
impl SyncEngine for MockSync {
    async fn create(&self, _name: &str, _alpha: &str, _beta: &str) -> Result<(), SyncError> {
        Ok(())
    }

    async fn pause(&self, _name: &str) -> Result<(), SyncError> {
        Ok(())
    }

    async fn resume(&self, _name: &str) -> Result<(), SyncError> {
        Ok(())
    }

    async fn terminate(&self, _name: &str) -> Result<(), SyncError> {
        Ok(())
    }

    async fn flush(&self, _name: &str) -> Result<(), SyncError> {
        Ok(())
    }

    async fn status(&self, _name: &str) -> Result<SyncStatus, SyncError> {
        Ok(SyncStatus::UpToDate)
    }

    async fn sessions(&self) -> Result<Vec<SessionInfo>, SyncError> {
        Ok(vec![])
    }

    async fn shutdown(&self) {}
}
