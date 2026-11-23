* [Features](README.md) - How feature plans are written, the frontmatter keys every document carries, and what the feature statuses mean.
* [Log](log.md) - dated record of feature status changes

## in-progress

* [fastest-time-to-wow](fastest-time-to-wow/index.md) - Reshape first-run onboarding around one metric, the fastest time to a local-model wow: story-voiced setup, a local model downloaded with consent, a pre-built hello-world sandbox with pi, and one consistent word for models.
* [github-cli-auth](github-cli-auth/index.md) - Log in to GitHub once through the GitHub CLI in the auth container, then apply that token to every sandbox's gh and git, at create and to existing sandboxes, with a per-sandbox opt-out.
* [host-tool-ssh-access](host-tool-ssh-access/index.md) - Let host SSH clients (e.g. the Claude Code desktop app) attach to a sandbox over the existing exec transport.
* [native-terminal](native-terminal/index.md) - Faster terminal startup: render the sandbox terminal in the main webview over a Rust-owned SSH session that attaches zellij, replacing the zellij web child webview.
* [onboarding-story](onboarding-story/index.md) - Illustrated onboarding book inside the welcome flow, extensible by chapter, with first-run setup running in the background and reported through the activity center.
* [retro-ui-migration](retro-ui-migration/index.md) - Re-skin the UI primitives with the retro-ui-svelte registry (Sunset Orange, rounded).
* [sandbox-resource-monitor](sandbox-resource-monitor/index.md) - Sample each local sandbox's CPU and memory, show a discrete health indicator with a restart hint, and keep a local usage history that tells the maintainer whether sandboxes need more default memory.
* [workspace-ux](workspace-ux/index.md) - Make the in-sandbox workspace friendlier for non-technical users by making the left app sidebar the only app chrome, so hosted apps feel like the app itself.

## planned

* [tailscale-access](tailscale-access/index.md) - Join deployed sandboxes to the user's tailnet so their web services are reachable from other devices.

## idea

* [chat-mcp-learning](chat-mcp-learning/index.md) - A built-in offline demo MCP server and MCP visibility in the native chat (server badges on tool calls, a tools popover), so users can learn what an MCP server does with the local model.
* [cloud-sandboxes](cloud-sandboxes/index.md) - Top-level Cloud Sandboxes nav panel for administering remote Podman hosts, including Oracle Cloud and Google Cloud provisioning.
* [mcp-servers](mcp-servers/index.md) - An app-wide library of MCP servers, added by pasting a URL or README snippet and switched on per sandbox, rendered into the native MCP config of Claude Code, OpenCode and pi so Paseo, the terminal and the native chat all use them.
* [proxy-remote-deploy](proxy-remote-deploy/index.md) - Let a proxied sandbox be deployed to a remote host; remote deploy is blocked for every sandbox until it lands.

## implemented

* [activity-center](activity-center/index.md) - App-wide registry and UI for long-running background tasks (downloads, installs, provisioning) with notifications.
* [agent-setup](agent-setup/index.md) - Curated catalogue of coding agents installed into a sandbox via mise, with a guided Claude Code auth flow.
* [agent-web-ui](agent-web-ui/index.md) - Browser-based agent UI (Paseo) for OpenCode, Pi and Claude Code as an alternative to the terminal TUI.
* [chat-agent-anatomy](chat-agent-anatomy/index.md) - A learning mode for the native chat that labels each part of a live agent run as Brain or Arms with a small animated avatar beside it, using the onboarding book's vocabulary.
* [consolidate-app-container](consolidate-app-container/index.md) - Collapse the AppContainer's chrome webviews into one NavigationPanel document.
* [credentials-inline-auth](credentials-inline-auth/index.md) - Claude Code OAuth login as an inline expandable card on the Credentials page instead of a separate window.
* [desktop-app-feel](desktop-app-feel/index.md) - Frontend-only UI/UX changes that make the app feel like a native desktop app.
* [e2e-tiering](e2e-tiering/index.md) - Split the e2e suite into light (mocked), standard (real Podman) and heavy (large assets) tiers.
* [embedded-windows](embedded-windows/index.md) - Host a sandbox's tool surfaces (terminal, apps) as child webviews inside the main window.
* [expose-ports](expose-ports/index.md) - Exposing sandbox ports on the host; predates the feature pipeline, so only deferred improvements are recorded.
* [herdr-multiplexer](herdr-multiplexer/index.md) - Replace zellij with Herdr as the in-sandbox terminal multiplexer behind the native terminal, for its mouse-first interface and built-in coding-agent awareness.
* [lib-decomposition](lib-decomposition/index.md) - Split the 1,800-line src-tauri/src/lib.rs into command, service and startup modules.
* [loading-states](loading-states/index.md) - Per-row in-flight feedback (loading buttons, StatusRow, action runner) for every sandbox action.
* [local-llm](local-llm/index.md) - Detect host specs, recommend and download a local model, and run a host inference server.
* [local-llm-reachability](local-llm-reachability/index.md) - Make the host inference server reachable from every sandbox, microsandbox included, through a per-sandbox loopback forward, and keep every local-model agent's provisioned config current.
* [manage-dashboard-redesign](manage-dashboard-redesign/index.md) - Make a sandbox's Manage dashboard open instantly and read simply: an Overview for launching and agents, advanced controls in a section sidebar that loads on demand.
* [models-page-caching](models-page-caching/index.md) - Make the /models route instant on repeat visits by caching the host-spec probe.
* [mutagen-transport](mutagen-transport/index.md) - Provider-neutral port forwarding and workspace transport over the Mutagen sidecar.
* [native-agent-ui](native-agent-ui/index.md) - Built-in chat UI in the app's own frontend, driving pi inside the sandbox over its RPC mode, kept alive by an in-sandbox supervisor.
* [nav-responsive](nav-responsive/index.md) - Responsive layout for the NavigationColumn's collapsed rail and expanded sidebar.
* [network-proxy](network-proxy/index.md) - Per-sandbox mitmproxy sidecar for outbound traffic inspection and secret injection.
* [opencode-agent](opencode-agent/index.md) - OpenCode as an installable agent, pre-wired to the host local-LLM inference server.
* [package-autocomplete](package-autocomplete/index.md) - Registry-backed suggestion dropdown for the sandbox package install input.
* [parallel-worktree-e2e](parallel-worktree-e2e/index.md) - Run the e2e light tier concurrently from several git worktrees, isolated by a per-worktree run slug.
* [paseo-plugins](paseo-plugins/index.md) - Ship app-owned Paseo plugins in the binary and register them with each sandbox's Paseo daemon at launch, to extend the Agents pane.
* [pi-agent](pi-agent/index.md) - pi as a third installable agent, pre-wired to the host local-LLM inference server.
* [podman-egress-boundary](podman-egress-boundary/index.md) - Fail-closed egress for Podman sandboxes: an internal Podman network with the proxy sidecar as the only exit, and iron-proxy replacing the mitmproxy sidecar for policy, secret swap and the traffic record.
* [podman-exec-transport](podman-exec-transport/index.md) - Reach containers over an SSH-over-exec transport instead of publishing ports.
* [podman-process-isolation](podman-process-isolation/index.md) - Stop spawned processes on Windows from inheriting the app's console handles.
* [podman-startup-recovery](podman-startup-recovery/index.md) - Graceful recovery when the Podman machine is stopped or its socket is missing after a host restart.
* [proxy-credential-secrets](proxy-credential-secrets/index.md) - Keep the GitHub token on the host: sandboxes hold a fixed placeholder that the iron-proxy sidecar swaps for the real token on GitHub requests.
* [proxy-traffic-viewer](proxy-traffic-viewer/index.md) - Per-sandbox panel listing the HTTP(S) flows the proxy sidecar observed.
* [remote-deploys](remote-deploys/index.md) - Deploy a sandbox from the local Podman machine to a remote Podman host.
* [remote-sync](remote-sync/index.md) - Run a sandbox on a remote host while mirroring its workspace to the laptop with Mutagen.
* [remote-terminal](remote-terminal/index.md) - The in-app Zellij web terminal for sandboxes deployed on a remote host, via a Mutagen forward.
* [remove-microsandbox](remove-microsandbox/index.md) - Remove the microsandbox provider and make Podman the only provider, keeping the provider, capability, process and net-policy seams so a future provider is a small addition.
* [remove-mitmproxy](remove-mitmproxy/index.md) - Remove the legacy pre-boundary mitmproxy sidecar path, keeping ProxyEngine with one variant as the seam for another proxy engine.
* [responsive-panel](responsive-panel/index.md) - Make the embedded workspace's global panel content adapt to narrow widths.
* [sandbox-list-create](sandbox-list-create/index.md) - Inline create-sandbox item at the bottom of the sandbox list, with an existing-folder option.
* [sandbox-runtime-management](sandbox-runtime-management/index.md) - Record installed software with mise and reproduce it on a remote host at deploy.
* [scriptable-mock-backend](scriptable-mock-backend/index.md) - Runtime-scriptable mocks so light e2e specs can simulate failures and non-happy states.
* [shared-agent-auth](shared-agent-auth/index.md) - Host-side credential store so an agent login is done once and reused across sandboxes.
* [techdebt-sweep](techdebt-sweep/index.md) - Resolve the open tech debt found by the 2026-11-07 audit: msb build locks, duplicated Podman bring-up, the legacy relay path, secrets on disk and in logs, unbounded calls.
* [unified-sidebar-nav](unified-sidebar-nav/index.md) - One persistent left sidebar replaces the top page tabs and the multi-sandbox switcher bar, pages use the full content area, and navigation labels speak plain language.
* [web-terminal](web-terminal/index.md) - Interactive Zellij web terminal for each sandbox, shown inside the app.
* [welcome-onboarding](welcome-onboarding/index.md) - First-run hero and onboarding flow that sets up the VM and base image.
* [workspace-app-tabs](workspace-app-tabs/index.md) - Open a sandbox on a Manage launcher tab and let the user open app tabs from it.

## superseded

* [microsandbox-network-policy](microsandbox-network-policy/index.md) - Native egress policy, host-held secrets and a chained traffic proxy for microsandbox sandboxes.
* [microsandbox-provider](microsandbox-provider/index.md) - microsandbox (libkrun microVMs via the msb CLI) as a second machine provider alongside Podman.
* [proxy-network-policy](proxy-network-policy/index.md) - Domain allow/deny and interactive approval for Podman sandboxes through the mitmproxy sidecar.
* [proxy-non-http-capture](proxy-non-http-capture/index.md) - Surface connections that bypass the proxy (non-HTTP(S) traffic) in the proxy panel.

## abandoned

* [proxy-toggle-management](proxy-toggle-management/index.md) - App-global proxy default and turning the proxy on or off for an existing sandbox by recreating it.
