//! The app-wide concepts every layer may use: the mock-mode switch, sandbox identity (naming rules
//! and workspace paths), the [`types::Sandbox`] data type, [`types::ProviderId`] and
//! [`types::BackendError`], the shared error currency. Depends on no subsystem, so a subsystem can name a sandbox or fail with a
//! `BackendError` without depending on `backends/`.

pub mod types;

use mio_env::env_dir;
use types::BackendError;

/// The container-name suffix that marks a Podman container as a proxy sidecar rather than a
/// sandbox — filtered out of `list_sandboxes` (`commands/sandbox.rs`) the same way
/// `credentials::types::AUTH_CONTAINER_PREFIX` filters out throwaway auth containers. Lives here,
/// not in `proxy/`, because [`validate_new_name`] reserves it.
pub const PROXY_CONTAINER_SUFFIX: &str = "-proxy";

/// Whether `MIO_MOCK_BACKEND=1` selects the scripted/in-memory subsystem paths (the e2e `light`
/// tier).
pub fn mock_backend_enabled() -> bool {
    std::env::var("MIO_MOCK_BACKEND").ok().as_deref() == Some("1")
}

/// Directory under which name-derived workspaces are created.
pub fn workspace_root() -> Result<std::path::PathBuf, BackendError> {
    if let Some(root) = env_dir("SANDBOX_WORKSPACE_ROOT") {
        return Ok(root);
    }
    let documents = dirs::document_dir().ok_or(BackendError::DocumentsDirNotFound)?;
    Ok(documents.join("sandbox-mio"))
}

/// Rejects names Podman would refuse. Also the path-safety gate for `workspace_path`: the name
/// becomes a path segment, so barring `/` and `..` keeps auto-created workspaces inside the root.
pub fn validate_name(name: &str) -> Result<(), BackendError> {
    let valid = name
        .chars()
        .enumerate()
        .all(|(i, c)| c.is_ascii_alphanumeric() || (i > 0 && matches!(c, '_' | '.' | '-')));
    if name.is_empty() || !valid {
        return Err(BackendError::InvalidName { name: name.to_string() });
    }
    Ok(())
}

/// [`validate_name`] plus the checks that apply only to a name being newly claimed: the proxy
/// sidecar suffix is reserved, and the name has no dot because Mutagen rejects `.` in the session
/// names derived from it (`docs/features/namespaced-ssh-alias/02_dev.md`, Resolved Decision 5).
/// Names that predate these rules stay addressable through `validate_name`, which must not gain
/// them. The folder-mode name suggestion in `src/routes/projects/CreateSandboxItem.svelte` replaces
/// dots to stay valid.
pub fn validate_new_name(name: &str) -> Result<(), BackendError> {
    validate_name(name)?;
    if name.contains('.') {
        return Err(BackendError::NameHasDot { name: name.to_string() });
    }
    if name.ends_with(PROXY_CONTAINER_SUFFIX) {
        return Err(BackendError::ReservedName { name: name.to_string() });
    }
    Ok(())
}

/// Name-derived workspace path under `workspace_root()`; also the fallback for containers lacking
/// the workspace label (`PodmanProvider::list_instances`).
pub fn workspace_path(name: &str) -> Result<String, BackendError> {
    let path = workspace_root()?.join(name);
    Ok(path.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
mod new_name_tests {
    use super::*;

    /// `validate_new_name` is pure, so this does no IO.
    #[test]
    fn dotted_new_name_is_refused_but_still_valid() {
        assert!(matches!(validate_new_name("a.b"), Err(BackendError::NameHasDot { .. })));
        assert!(validate_name("a.b").is_ok());
    }
}
