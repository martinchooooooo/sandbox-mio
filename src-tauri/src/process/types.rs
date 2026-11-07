use std::process::Output;

/// Runs one external-tool invocation and returns its raw output. `args` excludes the binary name;
/// `args[0]` is the CLI subcommand (`"create"`, `"machine"`, `"sync"`, ...). Implemented once per
/// real tool ([`SystemPodmanRunner`]) and once per scripted tool (`ScriptedPodmanRunner` in
/// `src-tauri/src/mockctl/`), so the owning subsystem struct (`CliBackend`,
/// `PodmanMachineManager`) is identical in both production and the e2e `light` tier.
#[async_trait::async_trait]
pub trait ProcessRunner: Send + Sync {
    async fn run(&self, args: &[&str]) -> std::io::Result<Output>;

    /// [`Self::run`] with extra `(name, value)` pairs set in the child's environment only — the one
    /// way a secret value reaches `msb` without entering argv or this process's own environment
    /// (`docs/features/microsandbox-network-policy/02_dev.md`). A runner that does not override
    /// this refuses non-empty `env` rather than dropping it, so a secret is never silently lost.
    async fn run_with_env(
        &self,
        args: &[&str],
        env: &[(String, String)],
    ) -> std::io::Result<Output> {
        if env.is_empty() {
            return self.run(args).await;
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "this process runner cannot pass environment variables",
        ))
    }
}

/// Real `podman` invocations: wraps `podman_command_std()` (`src-tauri/src/backends/mod.rs`),
/// preserving its resource-dir/env resolution. Also the podman seam's single health verdict — see
/// [`super::breaker`] and `docs/features/podman-process-isolation/02_dev.md`, Phase D.
pub struct SystemPodmanRunner;

/// Real Mutagen sidecar invocations: wraps `sync::command()` (`src-tauri/src/sync/mod.rs`),
/// preserving its `MUTAGEN_DATA_DIRECTORY` isolation. Holds the [`tauri::AppHandle`] because the
/// sidecar is resolved through it, which is why this is a separate runner from
/// [`SystemPodmanRunner`].
pub struct SystemMutagenRunner {
    pub(super) app: tauri::AppHandle,
}

/// Real `msb` invocations, always against the staged copy of the bundled `msb` + `libkrunfw`
/// (`msb_command()` in `src-tauri/src/process/mod.rs`); never a host install. Each call is bounded
/// (`msb_call_timeout()`). The binary is bundled; the Windows Hypervisor Platform feature it needs is not, so that stays
/// a host prerequisite reported by `msb doctor`. See `docs/features/microsandbox-provider/02_dev.md`.
pub struct SystemMsbRunner;
