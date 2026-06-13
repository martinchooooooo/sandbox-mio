/// Internal helper trait shared by Podman backends. Not part of the public API.
///
/// New Podman backends should implement this too.
///
/// Env vars that control behaviour:
/// - `PODMAN_MACHINE` — Podman machine name (default: `podman-machine-default`; E2E tests set this to `mio-tests`)
/// - `PODMAN_PIPE` — overrides the full named-pipe path; skips `podman_pipe()` derivation entirely
pub(crate) trait PodmanBackend {
    /// Returns the Windows named-pipe path for the Podman socket.
    /// Override entirely by setting `PODMAN_PIPE`; otherwise derived from `podman_machine()`.
    fn podman_pipe(&self) -> String {
        if let Ok(pipe) = std::env::var("PODMAN_PIPE") {
            return pipe;
        }
        let mut machine = self.podman_machine();
        // Only podman-machine-default seems to carry the extra prefix
        if !machine.starts_with("podman-") {
            machine = format!("podman-{machine}")
        }
        format!("//./pipe/{}", machine)
    }

    /// Returns the Podman machine name, defaulting to `podman-machine-default`.
    /// Override by setting `PODMAN_MACHINE` (the E2E harness sets this to `mio-tests`).
    fn podman_machine(&self) -> String {
        std::env::var("PODMAN_MACHINE")
            .unwrap_or_else(|_| "podman-machine-default".to_string())
    }

    /// Base image used when creating sandboxes. Override in a subtype to change the image.
    fn base_image() -> &'static str { "alpine" }
    /// Tag for `base_image`. Override in a subtype to pin a specific version.
    fn base_image_version() -> &'static str { "latest" }
}
