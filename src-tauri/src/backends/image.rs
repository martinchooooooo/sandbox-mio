//! The per-workspace marker for a dismissed base-image update prompt.

use crate::domain::types::BackendError;

const DISMISSED_MARKER: &str = "dismissed_image_version";

/// The `provider::image::BASE_IMAGE_VERSION` a workspace's user dismissed the update prompt for.
/// Absent or unreadable means nothing was dismissed, so the prompt shows.
pub fn dismissed_image_version(workspace_path: &str) -> Option<u32> {
    let path = crate::mio_files::mio_path(workspace_path, DISMISSED_MARKER);
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Record that the workspace's user dismissed the update prompt for `version`.
pub fn dismiss_image_update(workspace_path: &str, version: u32) -> Result<(), BackendError> {
    let path = crate::mio_files::mio_path(workspace_path, DISMISSED_MARKER);
    Ok(crate::mio_files::write_text(&path, &version.to_string())?)
}
