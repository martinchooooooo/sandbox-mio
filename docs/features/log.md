# Feature log

Records feature status changes only: creation (`Create`), reaching `implemented` (`Implement`),
`Supersede` and `Abandon`. Git is the record of everything else. Seeded on 2026-11-05 from git
history with at most one `Create` and one `Implement` entry per feature, so seeded dates are
approximate. New entries go on top.

## 2026-11-10

* **Implement** [techdebt-sweep](techdebt-sweep/index.md): Phases A–K, closed out by the `standard` tier run
* **Implement** [sandbox-runtime-management](sandbox-runtime-management/index.md): mise deploy verification passed (`runtime-management`, `deploy-remote`)
* **Implement** [local-llm-reachability](local-llm-reachability/index.md): per-sandbox inference forward for Podman and microsandbox, unified agent provisioning
* **Create** [local-llm-reachability](local-llm-reachability/index.md): ideation drafted

## 2026-11-09

* **Implement** [manage-dashboard-redesign](manage-dashboard-redesign/index.md): feat(dashboard): translate the Manage dashboard and close out the redesign

## 2026-11-08

* **Create** [manage-dashboard-redesign](manage-dashboard-redesign/index.md): ideation drafted

## 2026-11-07

* **Create** [techdebt-sweep](techdebt-sweep/index.md): plan recorded from the 2026-11-07 tech-debt audit

## 2026-11-02

* **Implement** [microsandbox-network-policy](microsandbox-network-policy/index.md): feat(msb): enforce network policy and chain a traffic proxy for microsandbox (`1e3d5df`)

## 2026-08-19

* **Implement** [proxy-traffic-viewer](proxy-traffic-viewer/index.md): feat(proxy): add per-sandbox traffic viewer (`4d6d76f`)

## 2026-08-18

* **Implement** [desktop-app-feel](desktop-app-feel/index.md): feat(desktop-app-feel): translate onboarding, auth, and settings copy (`b74cd21`)
* **Implement** [network-proxy](network-proxy/index.md): feat(proxy): add network proxy for sandboxed agent traffic (`5c81599`)
* **Create** [proxy-credential-secrets](proxy-credential-secrets/index.md): plan drafted
* **Create** [proxy-network-policy](proxy-network-policy/index.md): plan drafted
* **Create** [proxy-non-http-capture](proxy-non-http-capture/index.md): plan drafted
* **Create** [proxy-remote-deploy](proxy-remote-deploy/index.md): plan drafted
* **Create** [proxy-toggle-management](proxy-toggle-management/index.md): plan drafted

## 2026-08-17

* **Implement** [podman-process-isolation](podman-process-isolation/index.md): fix(podman): isolate spawned processes from the app's console handles (`74fef4a`)
* **Implement** [lib-decomposition](lib-decomposition/index.md): Refactor: Split up lib.rs (`47e02b3`)
* **Create** [expose-ports](expose-ports/index.md): docs(expose-ports): mark deferred improvements resolved, add upgrade regression test (`8a9e8a9`)
* **Create** [mutagen-transport](mutagen-transport/index.md): docs(feature): mutagen-transport already implemented (`d147250`)
* **Create** [host-tool-ssh-access](host-tool-ssh-access/index.md): plan drafted

## 2026-08-16

* **Create** [cloud-sandboxes](cloud-sandboxes/index.md): plan drafted

## 2026-08-14

* **Implement** [microsandbox-provider](microsandbox-provider/index.md): feat(provider): select the machine provider per sandbox (`670a0d5`)
* **Create** [microsandbox-provider](microsandbox-provider/index.md): docs(microsandbox): feature docs, e2e test plan, and spike records (`6253d98`)

## 2026-08-10

* **Implement** [podman-exec-transport](podman-exec-transport/index.md): feat(transport): reach exec-capable sandboxes via podman-exec SSH tunnel (`24d3f41`)

## 2026-08-03

* **Implement** [parallel-worktree-e2e](parallel-worktree-e2e/index.md): feat(e2e): isolate every e2e run per git worktree (`89a4841`)
* **Implement** [welcome-onboarding](welcome-onboarding/index.md): feat(welcome): add first-run onboarding flow with VM/base-image setup (`556045b`)

## 2026-07-26

* **Implement** [scriptable-mock-backend](scriptable-mock-backend/index.md): feat(e2e,mock): replace per-subsystem mocks with a scriptable process seam (`f66ed3d`)

## 2026-07-06

* **Implement** [nav-responsive](nav-responsive/index.md): ui: Navigation Column to be responsive (`c0fc6a8`)
* **Implement** [consolidate-app-container](consolidate-app-container/index.md): feat(workspace): consolidate NavigationPanel/Switcher/Dashboard into one webview (`1b9fd89`)
* **Implement** [credentials-inline-auth](credentials-inline-auth/index.md): feat(credentials): move Claude Code login into an inline expandable card (`8d7b509`)

## 2026-07-04

* **Implement** [workspace-app-tabs](workspace-app-tabs/index.md): feat(workspace): lazy Terminal/Agents panes behind a per-sandbox launcher (`95868e0`)

## 2026-07-03

* **Implement** [shared-agent-auth](shared-agent-auth/index.md): feat(agents): add shared credential store for one-time agent login (`957b636`)
* **Implement** [agent-web-ui](agent-web-ui/index.md): feature(agent-web-ui): Setup for having Paseo available (`45a5dd4`)
* **Implement** [sandbox-list-create](sandbox-list-create/index.md): feat(sandbox): create a sandbox from an existing directory (`945ed9a`)

## 2026-07-02

* **Implement** [embedded-windows](embedded-windows/index.md): feat(workspace): embed sandbox terminals as multi-webview panes (`17c78ed`)
* **Implement** [models-page-caching](models-page-caching/index.md): feat(models): cache machine specs probe across /models mounts (`5813ba0`)
* **Implement** [responsive-panel](responsive-panel/index.md): feat(workspace): give each sandbox a Terminal + Dashboard pane in the responsive panel (`37c80df`)

## 2026-07-01

* **Implement** [e2e-tiering](e2e-tiering/index.md): e2e: split spec suite into standard/light/heavy tiers with mock backend (`b3302bf`)
* **Implement** [opencode-agent](opencode-agent/index.md): feat(agents): add OpenCode agent pre-wired to local-LLM inference (`9d8fb45`)
* **Implement** [pi-agent](pi-agent/index.md): feat(agents): add pi coding agent pre-wired to local-LLM inference (`91fa4e8`)

## 2026-06-30

* **Implement** [activity-center](activity-center/index.md): feat(activity-center): global task tracking & notifications (`cd8c3da`)
* **Implement** [podman-startup-recovery](podman-startup-recovery/index.md): feat(podman-startup-recovery): graceful recovery for stopped machine (`2bec8f4`)

## 2026-06-29

* **Implement** [local-llm](local-llm/index.md): feat(local-llm): host inference engine with models & settings UI (`758d8f1`)
* **Create** [activity-center](activity-center/index.md): plan: activity center (`9e85cd9`)

## 2026-06-28

* **Implement** [package-autocomplete](package-autocomplete/index.md): feat(package-autocomplete): registry-backed suggestion dropdown + e2e tests (`dc5a21a`)
* **Implement** [agent-setup](agent-setup/index.md): feat(agent-setup): install + auth flow for Claude Code in sandbox (`0322d48`)

## 2026-06-27

* **Implement** [web-terminal](web-terminal/index.md): feat(web-terminal): in-app zellij terminal for local sandboxes (`4927af3`)
* **Implement** [remote-deploys](remote-deploys/index.md): fix(deploy): build base image on remote when absent before podman create (`7a3351a`)
* **Implement** [remote-terminal](remote-terminal/index.md): feat(remote-terminal): open Zellij terminal for deployed sandboxes via Mutagen forward (`ea337a8`)
* **Implement** [loading-states](loading-states/index.md): feat(loading-states): per-row in-flight feedback for all sandbox actions (`ab84349`)

## 2026-06-23

* **Create** [sandbox-runtime-management](sandbox-runtime-management/index.md): plans: Sandbox manifests with Flox (`606c45c`)

## 2026-06-21

* **Create** [remote-sync](remote-sync/index.md): Docs pattern for feature building (`267cada`)
