* [Features](README.md) - How feature plans are written, the frontmatter keys every document carries, and what the feature statuses mean.
* [Log](log.md) - dated record of feature status changes

## in-progress

* [retro-ui-migration](retro-ui-migration/index.md) - Re-skin the UI primitives with the retro-ui-svelte registry (Sunset Orange, rounded).
* [sandbox-runtime-management](sandbox-runtime-management/index.md) - Record installed software with mise and reproduce it on a remote host at deploy.
* [techdebt-sweep](techdebt-sweep/index.md) - Resolve the open tech debt found by the 2026-11-07 audit: msb build locks, duplicated Podman bring-up, the legacy relay path, secrets on disk and in logs, unbounded calls.

## planned

* [host-tool-ssh-access](host-tool-ssh-access/index.md) - Let host SSH clients (e.g. the Claude Code desktop app) attach to a sandbox over the existing exec transport.
* [proxy-credential-secrets](proxy-credential-secrets/index.md) - Source proxy secret-injection values from the credential store instead of hand-entered values.
* [proxy-network-policy](proxy-network-policy/index.md) - Domain allow/deny and interactive approval for Podman sandboxes through the mitmproxy sidecar.
* [proxy-non-http-capture](proxy-non-http-capture/index.md) - Surface connections that bypass the proxy (non-HTTP(S) traffic) in the proxy panel.
* [proxy-remote-deploy](proxy-remote-deploy/index.md) - Let a proxy-enabled sandbox be deployed to a remote host instead of being refused.
* [proxy-toggle-management](proxy-toggle-management/index.md) - App-global proxy default and turning the proxy on or off for an existing sandbox by recreating it.
* [tailscale-access](tailscale-access/index.md) - Join deployed sandboxes to the user's tailnet so their web services are reachable from other devices.

## idea

* [cloud-sandboxes](cloud-sandboxes/index.md) - Top-level Cloud Sandboxes nav panel for administering remote Podman hosts, including Oracle Cloud and Google Cloud provisioning.

## implemented

* [activity-center](activity-center/index.md) - App-wide registry and UI for long-running background tasks (downloads, installs, provisioning) with notifications.
* [agent-setup](agent-setup/index.md) - Curated catalogue of coding agents installed into a sandbox via mise, with a guided Claude Code auth flow.
* [agent-web-ui](agent-web-ui/index.md) - Browser-based agent UI (Paseo) for OpenCode, Pi and Claude Code as an alternative to the terminal TUI.
* [consolidate-app-container](consolidate-app-container/index.md) - Collapse the AppContainer's chrome webviews into one NavigationPanel document.
* [credentials-inline-auth](credentials-inline-auth/index.md) - Claude Code OAuth login as an inline expandable card on the Credentials page instead of a separate window.
* [desktop-app-feel](desktop-app-feel/index.md) - Frontend-only UI/UX changes that make the app feel like a native desktop app.
* [e2e-tiering](e2e-tiering/index.md) - Split the e2e suite into light (mocked), standard (real Podman) and heavy (large assets) tiers.
* [embedded-windows](embedded-windows/index.md) - Host a sandbox's tool surfaces (terminal, apps) as child webviews inside the main window.
* [expose-ports](expose-ports/index.md) - Exposing sandbox ports on the host; predates the feature pipeline, so only deferred improvements are recorded.
* [lib-decomposition](lib-decomposition/index.md) - Split the 1,800-line src-tauri/src/lib.rs into command, service and startup modules.
* [loading-states](loading-states/index.md) - Per-row in-flight feedback (loading buttons, StatusRow, action runner) for every sandbox action.
* [local-llm](local-llm/index.md) - Detect host specs, recommend and download a local model, and run a host inference server.
* [manage-dashboard-redesign](manage-dashboard-redesign/index.md) - Make a sandbox's Manage dashboard open instantly and read simply: an Overview for launching and agents, advanced controls in a section sidebar that loads on demand.
* [microsandbox-network-policy](microsandbox-network-policy/index.md) - Native egress policy, host-held secrets and a chained traffic proxy for microsandbox sandboxes.
* [microsandbox-provider](microsandbox-provider/index.md) - microsandbox (libkrun microVMs via the msb CLI) as a second machine provider alongside Podman.
* [models-page-caching](models-page-caching/index.md) - Make the /models route instant on repeat visits by caching the host-spec probe.
* [mutagen-transport](mutagen-transport/index.md) - Provider-neutral port forwarding and workspace transport over the Mutagen sidecar.
* [nav-responsive](nav-responsive/index.md) - Responsive layout for the NavigationColumn's collapsed rail and expanded sidebar.
* [network-proxy](network-proxy/index.md) - Per-sandbox mitmproxy sidecar for outbound traffic inspection and secret injection.
* [opencode-agent](opencode-agent/index.md) - OpenCode as an installable agent, pre-wired to the host local-LLM inference server.
* [package-autocomplete](package-autocomplete/index.md) - Registry-backed suggestion dropdown for the sandbox package install input.
* [parallel-worktree-e2e](parallel-worktree-e2e/index.md) - Run the e2e light tier concurrently from several git worktrees, isolated by a per-worktree run slug.
* [pi-agent](pi-agent/index.md) - pi as a third installable agent, pre-wired to the host local-LLM inference server.
* [podman-exec-transport](podman-exec-transport/index.md) - Reach containers over an SSH-over-exec transport instead of publishing ports.
* [podman-process-isolation](podman-process-isolation/index.md) - Stop spawned processes on Windows from inheriting the app's console handles.
* [podman-startup-recovery](podman-startup-recovery/index.md) - Graceful recovery when the Podman machine is stopped or its socket is missing after a host restart.
* [proxy-traffic-viewer](proxy-traffic-viewer/index.md) - Per-sandbox panel listing the HTTP(S) flows the proxy sidecar observed.
* [remote-deploys](remote-deploys/index.md) - Deploy a sandbox from the local Podman machine to a remote Podman host.
* [remote-sync](remote-sync/index.md) - Run a sandbox on a remote host while mirroring its workspace to the laptop with Mutagen.
* [remote-terminal](remote-terminal/index.md) - The in-app Zellij web terminal for sandboxes deployed on a remote host, via a Mutagen forward.
* [responsive-panel](responsive-panel/index.md) - Make the embedded workspace's global panel content adapt to narrow widths.
* [sandbox-list-create](sandbox-list-create/index.md) - Inline create-sandbox item at the bottom of the sandbox list, with an existing-folder option.
* [scriptable-mock-backend](scriptable-mock-backend/index.md) - Runtime-scriptable mocks so light e2e specs can simulate failures and non-happy states.
* [shared-agent-auth](shared-agent-auth/index.md) - Host-side credential store so an agent login is done once and reused across sandboxes.
* [web-terminal](web-terminal/index.md) - Interactive Zellij web terminal for each sandbox, shown inside the app.
* [welcome-onboarding](welcome-onboarding/index.md) - First-run hero and onboarding flow that sets up the VM and base image.
* [workspace-app-tabs](workspace-app-tabs/index.md) - Open a sandbox on a Manage launcher tab and let the user open app tabs from it.
