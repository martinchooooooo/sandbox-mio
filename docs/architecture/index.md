* [System Overview](overview.md) - What Sandbox-mio is, how its processes and subsystems fit together, where state lives, and the shared vocabulary; start here.
* [Coding Agents and Credentials](agents.md) - The agent catalogue, installing agents into sandboxes, logging in once and reusing the login everywhere, local-model wiring, and the Paseo agent web UI.
* [Local Inference](inference.md) - Running a language model on the host (hardware probe, recommendation, downloads, the llama.cpp and Foundry Local engines) and offering it to sandboxes.
* [Outbound Network Control](network.md) - Per-sandbox traffic viewer, secret injection and egress policy, and how Podman and microsandbox each implement them.
* [Providers, Machines and the Process Layer](providers.md) - The Podman and microsandbox engines behind one provider seam, how each is made ready on the host, and how every external process is run.
* [Remote Deploy and Sync](remote.md) - Moving a Podman sandbox to a remote Podman host while its workspace stays editable on the laptop: hosts, deploy, deployment state, undeploy and destroy.
* [Testing](testing.md) - The verification layers, the process-level mock seam behind the fast e2e tier, and how concurrent e2e runs stay isolated; spec-writing rules are in e2e/AGENTS.md.
* [Transport: SSH Aliases, Mutagen and Ports](transport.md) - How the app reaches services inside a sandbox: per-sandbox SSH aliases tunnelled through the engine's exec, Mutagen sync sessions and forwards, and port exposure.
* [Workspace UI: Window, Panes and Tabs](workspace-ui.md) - The single app window, the Rust-owned registry of embedded terminal and agent panes, the Manage tab and launchable apps, and terminal auto-login.

## In-flight plans

* [sandbox-runtime-management](../features/sandbox-runtime-management/index.md) - in-progress, changes providers.md, remote.md: Record installed software with mise and reproduce it on a remote host at deploy.
* [techdebt-sweep](../features/techdebt-sweep/index.md) - in-progress, changes providers.md, transport.md, agents.md, workspace-ui.md: Resolve the open tech debt found by the 2026-11-07 audit: msb build locks, duplicated Podman bring-up, the legacy relay path, secrets on disk and in logs, unbounded calls.
* [host-tool-ssh-access](../features/host-tool-ssh-access/index.md) - planned, changes transport.md: Let host SSH clients (e.g. the Claude Code desktop app) attach to a sandbox over the existing exec transport.
* [proxy-credential-secrets](../features/proxy-credential-secrets/index.md) - planned, changes network.md, agents.md: Source proxy secret-injection values from the credential store instead of hand-entered values.
* [proxy-network-policy](../features/proxy-network-policy/index.md) - planned, changes network.md: Domain allow/deny and interactive approval for Podman sandboxes through the mitmproxy sidecar.
* [proxy-non-http-capture](../features/proxy-non-http-capture/index.md) - planned, changes network.md: Surface connections that bypass the proxy (non-HTTP(S) traffic) in the proxy panel.
* [proxy-remote-deploy](../features/proxy-remote-deploy/index.md) - planned, changes network.md, remote.md: Let a proxy-enabled sandbox be deployed to a remote host instead of being refused.
* [proxy-toggle-management](../features/proxy-toggle-management/index.md) - planned, changes network.md, providers.md: App-global proxy default and turning the proxy on or off for an existing sandbox by recreating it.
* [tailscale-access](../features/tailscale-access/index.md) - planned, changes remote.md: Join deployed sandboxes to the user's tailnet so their web services are reachable from other devices.
* [cloud-sandboxes](../features/cloud-sandboxes/index.md) - idea, changes remote.md: Top-level Cloud Sandboxes nav panel for administering remote Podman hosts, including Oracle Cloud and Google Cloud provisioning.
