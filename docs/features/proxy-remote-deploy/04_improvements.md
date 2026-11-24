---
type: improvements
title: "Proxy Remote Deploy: Deferred Improvements"
feature: proxy-remote-deploy
---
# Proxy Remote Deploy: Deferred Improvements

Work noticed while planning [01_ideas.md](01_ideas.md) and [02_dev.md](02_dev.md) that is worthwhile but out of scope for deploying a bounded sandbox. An earlier version of this file was written for the removed mitmproxy sidecar; its "Switching an already-deployed sandbox's proxy toggle" entry is gone because `docs/features/proxy-toggle-management/` was abandoned and every sandbox now has a sidecar.

## Cross-connection sweep

`proxy::sweep` (`src-tauri/src/proxy/mod.rs`) scans only the local connection (`02_dev.md`, Resolved Decision 19). A crash or forced quit mid-deploy, after the remote sidecar exists but before the rollback runs, can leave `<sandbox>-proxy`, `mio-proxy-<sandbox>` or the sandbox container on the target with no Mutagen session pointing at them. A future sweep could enumerate registered connections and remove sidecars whose sandbox has no session there. Deferred because enumerating every connection at startup is unbounded and network-dependent, and the window is narrow.

## Air-gapped deploy targets

`02_dev.md`, Resolved Decision 11 pulls the iron-proxy image from Docker Hub on the target. A target without registry access could receive the image from the laptop with `podman image scp` or `podman save`/`load` over the connection. Deferred because `mise install` in `reproduce_remote` needs internet access on the target anyway, so an air-gapped target cannot deploy for other reasons.

## A real non-WSL remote host in the e2e harness

The `standard` tier's deploy target, `mio-tests-remote`, is a second WSL Podman machine sharing the local machine's image, so SELinux labelling, firewalld and older distribution Podman versions are never exercised (`02_dev.md`, Resolved Decision 7). A harness option pointing `REMOTE_CONNECTION` at a provisioned VM (for example a free-tier VM from `docs/features/cloud-sandboxes/01_ideas.md`) would close the gap. Deferred until such a host is available to the project.

## GitHub CLI login inside a deployed sandbox

`reconcile_locked` (`src-tauri/src/services/github_access.rs`) returns early for a deployed sandbox, so the fresh remote container never gets the gh placeholder login, although the remote sidecar holds the GitHub binding and mise's placeholder variable works. Applying the login on the remote site would need `github::apply` against the site's provider. Deferred because it predates this feature and is independent of the boundary.

## One progress channel for deploy

`02_dev.md` wraps deploy in one activity task, but the iron-proxy image pull still reports as its own `proxy-image-pull` task and the remote base-image build reports nothing (`docs/features/remote-deploys/01_ideas.md`, Open Questions). Folding both into the deploy task's steps would show deploy as one sequence. Deferred because the base-image build has no progress source yet.

## SELinux labels for the remote workspace mount

`PodmanProvider::create_instance` (`src-tauri/src/provider/mod.rs`) bind-mounts the remote workspace without a `:Z` or `:z` label. On an SELinux-enforcing target the sandbox may be denied access to `/root/work`. The remote sidecar avoids the problem by holding no bind mounts (`02_dev.md`, Resolved Decision 8); the workspace mount predates this feature. Deferred until Phase A check 4 shows whether it bites.

## Measuring the Podman 4.0 floor

The preflight accepts Podman 4.0 or newer with netavark (`02_dev.md`, Resolved Decision 6) on the strength of the upstream documentation; only Podman 5.8.6 was run (`spike/README.md`, check 4 not run). A measured run on a 4.x host, ideally with SELinux enforcing and firewalld, would confirm or raise the floor. Deferred until such a host is available.
