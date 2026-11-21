# Feature log

Records feature status changes only: creation (`Create`), reaching `implemented` (`Implement`),
`Supersede` and `Abandon`. Git is the record of everything else. Seeded on 2026-11-05 from git
history with at most one `Create` and one `Implement` entry per feature, so seeded dates are
approximate. New entries go on top.

## 2026-11-22

* **Implement** [chat-agent-anatomy](chat-agent-anatomy/index.md): the native chat gains a per-sandbox learning mode that outlines parts as Brain or Arms, splits tool calls into ask and act, and plays a live brain-and-arms stage in a companion panel; translations (Phase G) wait for approval

## 2026-11-21

* **Create** [chat-agent-anatomy](chat-agent-anatomy/index.md): ideation drafted; a learning mode for the native chat that outlines parts as Brain or Arms, splits tool calls into ask and act, and plays a live brain-and-arms stage in a left panel, reusing the onboarding stage engine; on by default in `hello-world`
* **Implement** [remove-microsandbox](remove-microsandbox/index.md): the microsandbox provider, its proxy path, bundling and UI are removed and Podman is the only provider, keeping the provider, capability, process and net-policy seams; the standard tier passes apart from load-flaky specs that pass on rerun
* **Supersede** [microsandbox-provider](microsandbox-provider/index.md): replaced by [remove-microsandbox](remove-microsandbox/index.md); microsandbox only ever added maintenance on top of Podman, which it needed to build its image
* **Supersede** [microsandbox-network-policy](microsandbox-network-policy/index.md): replaced by [remove-microsandbox](remove-microsandbox/index.md); the Podman iron-proxy sidecar enforces the shared policy format alone
* **Create** [remove-microsandbox](remove-microsandbox/index.md): ideation drafted; delete the microsandbox provider and make Podman the only provider while keeping the provider, capability, process and net-policy seams; no spike run
* **Implement** [paseo-plugins](paseo-plugins/index.md): app-owned plugins are bundled into the binary, delivered on Agents-tab open and registered, reloaded or pruned in the daemon config; the standard spec passes on Podman, and the microsandbox and deployed-sandbox checks were skipped by the user

## 2026-11-19

* **Implement** [proxy-credential-secrets](proxy-credential-secrets/index.md): the GitHub token stays on the host; sidecar-mode sandboxes hold a placeholder the iron-proxy sidecar swaps on GitHub requests, for gh, git and mise, and the Manage card shows the mode; the e2e specs in `03_e2e.md` are not implemented yet

## 2026-11-18

* **Create** [proxy-credential-secrets](proxy-credential-secrets/index.md): ideation rewritten from scratch; sandboxes hold a fixed GitHub-shaped placeholder that the iron-proxy sidecar swaps for the host-held token; the gh and git swap spike passed against iron-proxy 0.56.0; the earlier Claude-bundle plan was dropped
* **Create** [mcp-servers](mcp-servers/index.md): ideation drafted; an app-wide MCP library filled by pasting a URL or README snippet, switched on per sandbox and rendered into Claude Code, OpenCode and pi configs; gating spike not yet run
* **Create** [chat-mcp-learning](chat-mcp-learning/index.md): ideation drafted; a built-in offline demo MCP server and MCP visibility in the native chat, building on mcp-servers; gating spike not yet run
* **Create** [paseo-plugins](paseo-plugins/index.md): ideation drafted; ship app-owned Paseo plugins in the binary, stream them into each sandbox and register them in the daemon's `config.json` before launch; the delivery and hot-reload spike passed against Paseo 0.16.1
* **Create** [fastest-time-to-wow](fastest-time-to-wow/index.md): ideation drafted from the Expert UX review; local-model-first onboarding judged by fastest time to wow, with a consented model download, a hello-world sandbox with pi, and one word for models
* **Create** [github-cli-auth](github-cli-auth/index.md): ideation drafted; log in to GitHub once via gh's device flow in the auth container and apply the token to every sandbox, with a per-sandbox opt-out; the no-TTY login spike passed
* **Implement** [herdr-multiplexer](herdr-multiplexer/index.md): Herdr replaces zellij in the base image and the native terminal's two command strings; standard specs pass and the user checked the manual renderer, microsandbox and deployed-sandbox cases

## 2026-11-17

* **Create** [herdr-multiplexer](herdr-multiplexer/index.md): replace zellij with Herdr as the in-sandbox multiplexer behind the native terminal, for its mouse-first interface and agent awareness
* **Create** [sandbox-resource-monitor](sandbox-resource-monitor/index.md): per-sandbox CPU and memory sampling, a health indicator with a restart hint, and a local usage history for sizing the default sandbox memory
* **Implement** [unified-sidebar-nav](unified-sidebar-nav/index.md): one persistent sidebar replaces the top tabs and switcher bar; the user approved the new shell, and the `heavy` tier was not run before closing
* **Implement** [native-agent-ui](native-agent-ui/index.md): built-in pi chat pane over an in-sandbox supervisor, translated into all locales; the microsandbox hand check was skipped by the user
* **Supersede** [proxy-non-http-capture](proxy-non-http-capture/index.md): replaced by [podman-egress-boundary](podman-egress-boundary/index.md); a bounded Podman sandbox has no route around the sidecar, so there is no bypass left to count

## 2026-11-16

* **Create** [native-terminal](native-terminal/index.md): ideation drafted, gating spike not yet run

## 2026-11-15

* **Implement** [podman-egress-boundary](podman-egress-boundary/index.md): bounded Podman sandboxes behind an iron-proxy sidecar, with live policy apply and audit-fed traffic; Paraglide pass pending user approval

## 2026-11-13

* **Supersede** [proxy-network-policy](proxy-network-policy/index.md): replaced by [podman-egress-boundary](podman-egress-boundary/index.md); interactive approval deferred
* **Create** [podman-egress-boundary](podman-egress-boundary/index.md): ideation drafted, gating spikes not yet run
* **Create** [unified-sidebar-nav](unified-sidebar-nav/index.md): ideation drafted from the user-approved design

## 2026-11-11

* **Create** [native-agent-ui](native-agent-ui/index.md): ideation drafted
* **Create** [onboarding-story](onboarding-story/index.md): build plan recorded, Phases A–E built

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
