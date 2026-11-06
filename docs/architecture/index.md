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
* [tailscale-access](../features/tailscale-access/index.md) - planned, changes remote.md: Join deployed sandboxes to the user's tailnet so their web services are reachable from other devices.
