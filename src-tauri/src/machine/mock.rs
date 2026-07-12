//! In-memory mock of [`MachineManager`], gated by `MIO_MOCK_BACKEND=1` (see
//! [`super::default_machine_manager`]). Reports the default machine as always running and ready,
//! and every lifecycle action as a no-op success, so the e2e "light" test tier can drive
//! `check_podman_ready` and `start_podman_machine` without a real Podman machine or WSL.

use crate::backends::types::BackendError;

use super::types::MachineManager;

pub(super) struct MockMachineManager;

#[async_trait::async_trait]
impl MachineManager for MockMachineManager {
    async fn machine_is_running(&self) -> Result<bool, BackendError> {
        Ok(true)
    }

    async fn check_other_machines_running(&self) -> bool {
        false
    }

    async fn run_machine_start(&self) -> Result<(), BackendError> {
        Ok(())
    }

    async fn run_wsl_shutdown(&self) -> Result<(), BackendError> {
        Ok(())
    }

    async fn activate_podman_socket(&self) -> Result<(), BackendError> {
        Ok(())
    }

    async fn poll_ready(&self, _attempts: usize) -> Result<(), BackendError> {
        Ok(())
    }

    async fn wsl_is_ready(&self) -> bool {
        true
    }

    async fn install_wsl(&self) -> Result<bool, BackendError> {
        Ok(false)
    }

    async fn machine_exists(&self) -> Result<bool, BackendError> {
        Ok(true)
    }

    async fn init_machine(&self) -> Result<(), BackendError> {
        Ok(())
    }

    async fn reset_stuck_machine(&self) -> Result<(), BackendError> {
        Ok(())
    }
}
