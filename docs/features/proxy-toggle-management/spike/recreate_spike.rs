// SPIKE — throwaway, per docs/features/proxy-toggle-management/01_ideas.md's Validation section.
// Exercises the real `PodmanProvider::create_instance`/`remove`, `ensure_sandbox_alias`,
// `forget_sandbox_host_key`, and `RuntimeManager::reproduce_remote`/`prepare_terminal` this
// feature's design reuses, against a throwaway `mio-spike-toggle-test` sandbox on the real,
// currently-running `podman-machine-default`.
//
// This file is NOT part of the app build. It is a copy of a `#[cfg(test)] mod recreate_spike`
// that was temporarily appended to the bottom of `src-tauri/src/backends/mod.rs`, run once against
// real infrastructure, and then removed from that file (see `../01_ideas.md`'s Validation section
// for the run date and outcome). Unlike the Windows-process-isolation spike probes elsewhere under
// `docs/features/*/spike/`, this one cannot be compiled standalone with a bare `rustc` invocation —
// it depends on `sandbox_mio_lib`'s private (`pub(crate)`/private) items (`CliBackend::ensure_key`,
// `connection_name`, `base_image`, `forget_sandbox_host_key`, `ensure_sandbox_alias`,
// `crate::process::detached_command`), which only compile from inside the crate. To re-run it:
// paste this module back onto the end of `src-tauri/src/backends/mod.rs` and run
// `cargo test --lib backends::recreate_spike -- --ignored --nocapture` with
// `podman-machine-default` running, then remove it again afterward.
//
// Cleans up its container, ssh alias block, known_hosts entry, and temp workspace/identity
// directories on every exit path (success, assertion failure, or early return) via an explicit
// teardown step that always runs before the test's result is unwrapped, whether or not the run
// body succeeded.
#[cfg(test)]
mod recreate_spike {
    use std::sync::Arc;

    use super::{base_image, connection_name, ensure_sandbox_alias, forget_sandbox_host_key, CliBackend};
    use crate::process::types::{ProcessRunner, SystemMsbRunner, SystemPodmanRunner};
    use crate::provider::types::{ExecOpts, InstanceSpec, MachineProvider, MachineRef, ProviderId, PublishModel};
    use crate::provider::{default_provider, default_registry, WORKSPACE_LABEL};
    use crate::runtime::default_runtime;
    use crate::transport::default_port_transport;

    const SPIKE_NAME: &str = "mio-spike-toggle-test";

    fn alias_config_path() -> std::path::PathBuf {
        dirs::home_dir().expect("home dir resolvable").join(".ssh").join("sandbox-mio").join("config")
    }

    fn read_alias_block(name: &str) -> Option<String> {
        let text = std::fs::read_to_string(alias_config_path()).ok()?;
        let begin = format!("# >>> sandbox-mio {name} >>>");
        let end = format!("# <<< sandbox-mio {name} <<<");
        let start = text.find(&begin)?;
        let stop = text[start..].find(&end)? + start + end.len();
        Some(text[start..stop].to_string())
    }

    async fn install_key(
        provider: &Arc<dyn MachineProvider>,
        identity_path: &str,
    ) -> Result<(), String> {
        // Mirrors `CliBackend::install_sandbox_key` exactly (pubkey to a temp file, `copy_in` to the
        // fixed authorized-keys path `sshd_config.container` names) via the real `copy_in`, without
        // needing a `CliBackend`/`AppHandle` to call the private method through.
        let pub_key =
            std::fs::read_to_string(format!("{identity_path}.pub")).map_err(|e| e.to_string())?;
        let tmp = std::env::temp_dir().join("mio-spike-toggle-test-authorized-keys");
        std::fs::write(&tmp, &pub_key).map_err(|e| e.to_string())?;
        let result = provider
            .copy_in(SPIKE_NAME, &tmp.to_string_lossy(), "/etc/ssh/authorized_keys.container")
            .await
            .map_err(|e| e.to_string());
        let _ = std::fs::remove_file(&tmp);
        result
    }

    async fn ssh_probe(identity_path: &str) -> Result<(), String> {
        let _ = identity_path; // IdentityFile comes from the alias block itself, not passed here.
        let config = alias_config_path();
        let out = crate::process::detached_command("ssh")
            .arg("-F")
            .arg(&config)
            .arg("-o")
            .arg("BatchMode=yes")
            .arg("-o")
            .arg("ConnectTimeout=20")
            .arg(SPIKE_NAME)
            .arg("echo mio-spike-ssh-ok")
            .output()
            .await
            .map_err(|e| e.to_string())?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        if !out.status.success() || !stdout.contains("mio-spike-ssh-ok") {
            return Err(format!(
                "ssh probe failed: status={:?} stdout={stdout:?} stderr={:?}",
                out.status.code(),
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        Ok(())
    }

    async fn run_spike() -> Result<(), String> {
        let connection = connection_name();
        let runner: Arc<dyn ProcessRunner> = Arc::new(SystemPodmanRunner);
        let provider = default_provider(runner.clone(), connection.clone());

        // --- workspace + identity setup, entirely outside anything CliBackend/AppHandle owns ---
        let workspace_dir = std::env::temp_dir().join("mio-spike-toggle-test-workspace");
        let _ = std::fs::remove_dir_all(&workspace_dir);
        std::fs::create_dir_all(&workspace_dir).map_err(|e| e.to_string())?;
        let marker = workspace_dir.join("before-recreate.txt");
        std::fs::write(&marker, "spike marker written before recreate\n").map_err(|e| e.to_string())?;
        let workspace_source = dunce::canonicalize(&workspace_dir)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");

        let identity_dir = std::env::temp_dir().join("mio-spike-toggle-test-ssh-keys");
        let _ = std::fs::remove_dir_all(&identity_dir);
        std::fs::create_dir_all(&identity_dir).map_err(|e| e.to_string())?;
        let identity_path = identity_dir.join("id_ed25519").to_string_lossy().replace('\\', "/");
        CliBackend::ensure_key(&identity_path).map_err(|e| e.to_string())?;

        // --- ensure base image exists (idempotent; almost certainly already built) ---
        provider.ensure_base_image().await.map_err(|e| e.to_string())?;

        // Seed mise.toml/mise.lock the way `services::sandbox::create` does for every real sandbox
        // (`RuntimeManager::init`, not something `CliBackend::create` itself calls) — needed so the
        // later `reproduce_remote` step has a manifest/lockfile to copy onto the recreated container.
        let providers = default_registry(runner.clone(), Arc::new(SystemMsbRunner));
        let runtime = default_runtime(providers.clone(), default_port_transport());
        let local_ref = MachineRef { provider: ProviderId::Podman, id: connection.clone() };
        runtime.init(&workspace_source).await.map_err(|e| e.to_string())?;

        // --- create #1 (the real PodmanProvider::create_instance) ---
        let spec = InstanceSpec {
            name: SPIKE_NAME.to_string(),
            image: base_image(),
            workspace_source: workspace_source.clone(),
            labels: vec![(WORKSPACE_LABEL.to_string(), workspace_source.clone())],
            publish_model: PublishModel::ExecTunnelled,
            env: Vec::new(),
            network: None,
        };
        provider.create_instance(&spec).await.map_err(|e| e.to_string())?;
        provider.start(SPIKE_NAME).await.map_err(|e| e.to_string())?;

        install_key(&provider, &identity_path).await?;
        forget_sandbox_host_key(&identity_path);
        ensure_sandbox_alias(SPIKE_NAME, &connection, &identity_path)
            .await
            .map_err(|e| e.to_string())?;

        let alias_before =
            read_alias_block(SPIKE_NAME).ok_or("alias block missing after first create")?;
        eprintln!("[spike] alias block after first create:\n{alias_before}");

        // Confirm SSH works pre-recreate.
        ssh_probe(&identity_path).await?;

        // Note a zellij session pre-recreate (RuntimeManager::prepare_terminal, the real Terminal
        // pane open path).
        let (_, port_before) = runtime
            .prepare_terminal(SPIKE_NAME, &local_ref, &workspace_source, true)
            .await
            .map_err(|e| e.to_string())?;
        eprintln!("[spike] pre-recreate terminal ready on port {port_before}");

        // --- recreate: podman rm --force, then create/start again against the SAME workspace_source ---
        provider.remove(SPIKE_NAME).await.map_err(|e| e.to_string())?;

        // Risk 1: the workspace directory + marker file must survive `podman rm --force`.
        if !marker.exists() {
            return Err("workspace marker missing on host after podman rm --force".to_string());
        }

        provider.create_instance(&spec).await.map_err(|e| e.to_string())?;
        provider.start(SPIKE_NAME).await.map_err(|e| e.to_string())?;

        // Confirm the marker is visible INSIDE the recreated container (the bind mount really
        // re-attached to the same host directory, not just present on the host).
        let cat = provider
            .exec(SPIKE_NAME, &["cat", "/root/work/before-recreate.txt"], ExecOpts::default())
            .await
            .map_err(|e| e.to_string())?;
        if !cat.status.success() || !String::from_utf8_lossy(&cat.stdout).contains("spike marker") {
            return Err(format!(
                "workspace marker not visible inside recreated container: {:?}",
                String::from_utf8_lossy(&cat.stdout)
            ));
        }

        // Re-provision exec transport exactly as CliBackend::create's exec_capable branch does.
        forget_sandbox_host_key(&identity_path);
        install_key(&provider, &identity_path).await?;
        ensure_sandbox_alias(SPIKE_NAME, &connection, &identity_path)
            .await
            .map_err(|e| e.to_string())?;

        // Risk 2: the alias's rendered content must be byte-identical before and after recreate.
        let alias_after =
            read_alias_block(SPIKE_NAME).ok_or("alias block missing after recreate")?;
        if alias_before != alias_after {
            return Err(format!(
                "alias block differs after recreate:\n--- before ---\n{alias_before}\n--- after ---\n{alias_after}"
            ));
        }
        eprintln!("[spike] alias block byte-identical after recreate");

        // Risk 3: a fresh SSH connection against the recreated container.
        ssh_probe(&identity_path).await?;

        // A fresh Terminal-pane-equivalent: reproduce the mise toolchain, then open a fresh zellij
        // session — mirrors `retoggle_proxy`'s step 6 then a user reopening a Terminal pane.
        runtime
            .reproduce_remote(SPIKE_NAME, &local_ref, &workspace_source)
            .await
            .map_err(|e| e.to_string())?;
        let (_, port_after) = runtime
            .prepare_terminal(SPIKE_NAME, &local_ref, &workspace_source, true)
            .await
            .map_err(|e| e.to_string())?;
        eprintln!("[spike] post-recreate terminal ready on port {port_after}");

        Ok(())
    }

    async fn teardown() {
        let connection = connection_name();
        let runner: Arc<dyn ProcessRunner> = Arc::new(SystemPodmanRunner);
        let provider = default_provider(runner, connection);
        let _ = provider.remove(SPIKE_NAME).await;
        let _ = crate::ssh::remove_alias(SPIKE_NAME);
        let identity_dir = std::env::temp_dir().join("mio-spike-toggle-test-ssh-keys");
        let identity_path =
            identity_dir.join("id_ed25519").to_string_lossy().replace('\\', "/");
        forget_sandbox_host_key(&identity_path);
        let _ = std::fs::remove_dir_all(&identity_dir);
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("mio-spike-toggle-test-workspace"));
        let _ = std::fs::remove_file(
            std::env::temp_dir().join("mio-spike-toggle-test-authorized-keys"),
        );
    }

    #[tokio::test]
    #[ignore = "needs a running podman-machine-default; touches ~/.ssh/sandbox-mio/config"]
    async fn recreate_preserves_workspace_alias_and_ssh() {
        let result = run_spike().await;
        teardown().await;
        result.unwrap();
    }
}
