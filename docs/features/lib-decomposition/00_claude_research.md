---
type: research
title: "lib.rs Decomposition — Research Notes"
feature: lib-decomposition
---
I've read lib.rs end-to-end plus the subsystem seams it composes. Here's what's in there and how I'd break it apart.

What the file holds today (1,805 lines)

┌───────────────────────────────────────────────────────────────────────────────────────────┬────────────┬──────────────────────────────────────────────────────────────────────┐
│                                          Concern                                          │   Weight   │                                Notes                                 │
├───────────────────────────────────────────────────────────────────────────────────────────┼────────────┼──────────────────────────────────────────────────────────────────────┤
│ 77 #[tauri::command] fns                                                                  │ ~1,150     │ ~25 are one-line delegations; ~10 carry real orchestration           │
│                                                                                           │ lines      │                                                                      │
├───────────────────────────────────────────────────────────────────────────────────────────┼────────────┼──────────────────────────────────────────────────────────────────────┤
│ Pane preparation (prepare_terminal_webview, prepare_agent_web_pane, read_paseo_password,  │ ~185 lines │ Forward opening, .mio/*.json writes, JS init-script generation,      │
│ fetch_paseo_server_info)                                                                  │            │ paseo /api/status polling                                            │
├───────────────────────────────────────────────────────────────────────────────────────────┼────────────┼──────────────────────────────────────────────────────────────────────┤
│ Sandbox addressing (sandbox_facts, resolve_machine, ensure_forward_alias)                 │ ~55 lines  │ Called as a fixed triple by 5 commands                               │
├───────────────────────────────────────────────────────────────────────────────────────────┼────────────┼──────────────────────────────────────────────────────────────────────┤
│ Agent config writers (provision_opencode, provision_pi)                                   │ ~114 lines │ Pure host-side JSON/TOML merge, no state at all                      │
├───────────────────────────────────────────────────────────────────────────────────────────┼────────────┼──────────────────────────────────────────────────────────────────────┤
│ init_logging                                                                              │ ~52 lines  │ Subscriber + panic hook                                              │
├───────────────────────────────────────────────────────────────────────────────────────────┼────────────┼──────────────────────────────────────────────────────────────────────┤
│ run() bootstrap                                                                           │ ~100 lines │ Runner construction, 15 .manage() calls, exit handler                │
├───────────────────────────────────────────────────────────────────────────────────────────┼────────────┼──────────────────────────────────────────────────────────────────────┤
│ ephemeral_port, misc                                                                      │ ~20 lines  │                                                                      │
└───────────────────────────────────────────────────────────────────────────────────────────┴────────────┴──────────────────────────────────────────────────────────────────────┘

Two measurable symptoms of the state problem: 87 State<'_, …> parameters across 77 commands, and 15 managed items that any command can reach into. The four pane commands alone declare 21 state parameters between them.

Target shape: three layers

commands/<domain>.rs   thin IPC skin — decode args, pluck the narrowest state, delegate, map to AppError
services/<flow>.rs     cross-subsystem orchestration (the layer allowed to compose subsystems)
<subsystem>/           unchanged: trait + real impl + mock, never depends on a sibling
lib.rs                 mod list, run(), nothing else (~60 lines)

Today AGENTS.md says "subsystems never depend on each other" and orchestration lives in lib.rs. That rule is right; services/ just gives that orchestration a name and a home instead of a 1,800-line drawer.

Phase 0 — spike (30 min, do first)

generate_handler! has to move out of the builder call. The pattern is:

pub fn handler<R: tauri::Runtime>() -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![sandbox::create_sandbox, panes::open_terminal,
}

Worth compiling with three commands before committing to the split — if the generic form fights the macro, the fallback is keeping the 77-name list in lib.rs with commands::… paths, which still gets lib.rs to ~140 lines.

Phase 1 — extract non-command logic (no signature changes)

Pure moves, lib.rs → ~1,300 lines:

- services/panes.rs ← prepare_terminal_webview, prepare_agent_web_pane, read_paseo_password, fetch_paseo_server_info. These are the largest non-command bodies and they're genuine
orchestration (runtime + network + fs + HTTP).
- services/routing.rs ← sandbox_facts, resolve_machine, ensure_forward_alias. See Phase 4 — this is where the router type lands.
- agents/config.rs ← provision_opencode, provision_pi. No state, no cross-subsystem work; they belong to the agents subsystem outright.
- logging.rs ← init_logging.
- inference/mod.rs ← ephemeral_port (its error type is already InferenceError).
- backends/mod.rs ← the .mio/dismissed_image_version read/write bodies, next to BASE_IMAGE_VERSION which they gate on.

Phase 2 — split the commands

commands/mod.rs + 13 files. Every one is a domain a future change would target:

┌──────────────┬──────────┬────────────────┬──────────┐
│     File     │ Commands │      File      │ Commands │
├──────────────┼──────────┼────────────────┼──────────┤
│ panes.rs     │ 11       │ agents.rs      │ 7        │
├──────────────┼──────────┼────────────────┼──────────┤
│ inference.rs │ 10       │ sandbox.rs     │ 6        │
├──────────────┼──────────┼────────────────┼──────────┤
│ machine.rs   │ 8        │ deploy.rs      │ 6        │
├──────────────┼──────────┼────────────────┼──────────┤
│ image.rs     │ 6        │ credentials.rs │ 6        │
├──────────────┼──────────┼────────────────┼──────────┤
│ packages.rs  │ 4        │ hosts.rs       │ 4        │
├──────────────┼──────────┼────────────────┼──────────┤
│ app.rs       │ 4        │ network.rs     │ 3        │
├──────────────┼──────────┼────────────────┼──────────┤
│ mockctl.rs   │ 2        │                │ 77 total │
└──────────────┴──────────┴────────────────┴──────────┘

The handler list moves to commands/mod.rs as the single inventory. lib.rs → ~150 lines.

Phase 3 — extract bootstrap

bootstrap.rs with build_runners(), manage_state(app), on_exit(handle, eventve verbatim and get comments saying so:

- process::clear_inheritable_handles() stays the first statement of run() — it must precede everything, so it doesn't move into bootstrap.
- backends::set_podman_runner still precedes all Tauri state wiring (the PO a documented gotcha).

lib.rs → ~60 lines: mod declarations, run(), the devtools cfg block.

Phase 4 — narrow the state (the main goal)

Introduce one type in services/routing.rs:

pub struct SandboxRoute {
    pub provider: ProviderId,
    pub machine: MachineRef,
    pub exec_capable: bool,
    pub forward_alias: String,
}

pub struct SandboxRouter { store, engine, registry }   // managed once at setup
impl SandboxRouter {
    pub async fn resolve(&self, name: &str, app: &AppHandle) -> Result<SandboxRoute, AppError>;
    pub async fn locate(&self, name: &str) -> Result<(ProviderId, MachineRef, bool), AppError>; // no alias write
}

That single call replaces the sandbox_facts → resolve_machine → ensure_forward_alias triple at 5 sites, and the if exec_capable { ensure_forward_alias(…) } conditional duplicated in open_workspace and expose_port.

Effect on the worst signatures:

┌────────────────────┬────────────┬──────────────────────────────────────────┐
│      Command       │ States now │                  After
├────────────────────┼────────────┼──────────────────────────────────────────┤
│ open_agent_web     │ 6          │ 4 (router, runtime, network, workspaces) │
├────────────────────┼────────────┼──────────────────────────────────────────┤
│ open_terminal_pane │ 6          │ 4                                        │
├────────────────────┼────────────┼──────────────────────────────────────────┤
│ open_terminal      │ 5          │ 3                                        │
├────────────────────┼────────────┼──────────────────────────────────────────┤
│ open_workspace     │ 4          │ 2                                        │
├────────────────────┼────────────┼──────────────────────────────────────────┤
│ expose_port        │ 4          │ 2                                        │
└────────────────────┴────────────┴────────────────────────────────────────

The governing rule to write into AGENTS.md: a command names the narrowest handle that can do its job. Thin delegations keep their single Arc<dyn Trait>; multi-subsystem commands
take one service handle. No god object.

Optional further step: fold runtime + network into a PaneService alongside the router, taking the pane commands to 1–2 states. I'd hold this until Phase 4 lands — the router alone captures most of the win, and a second facade risks abstraction for its own sake.

Phase 5 — cover the repeated patterns

┌────────────────────────────────────────────────────────┬──────────────────────┬──────────────────────────────────────────────────────────────────────────────────────────────┐
│                        Pattern                         │   Count in lib.rs    │                                        Proposed cover                                        │
├────────────────────────────────────────────────────────┼────────────────────────────────────────────────────────────────────────────────────────┤
│                                                        │ 28 calls across 4    │ activity::run(&app, id, kind, target, |task| async { … }) — scope helper that resolves       │
│ activity::open → progress ×N → fail/succeed            │ commands        Result. Kills ~6 activity::fail(…); return Err(…) pairs in             │
│                                                        │                      │ start_podman_machine/provision_podman                                                        │
├────────────────────────────────────────────────────────┼──────────────────────┼──────────────────────────────────────────────────────────────────────────────────────────────┤
│ Channel that mirrors setup events into activity        │ 1 (20 lines, in      │ activity::bridge_channel(app, id, total_steps, downstream) — reusable for any future stepped │
│ progress                                               │ start_inference)     │  command                                                                                     │
├────────────────────────────────────────────────────────┼──────────────────────┼──────────────────────────────────────────────────────────────────────────────────────────────┤
│ BackendError::CommandFailed(format!("failed to write   │ 14              _map(path), merge_json(path, f), write_json_pretty(path, v),           │
│ {path}: {e}"))                                         │                      │ mio_dir(workspace) — path baked into the error once                                          │
├────────────────────────────────────────────────────────┼──────────────────────┼──────────────────────────────────────────────────────────────────────────────────────────────┤
│ .map_err(|e|                                           │ 8               eError in runtime/types.rs (::terminal(e), ::agent_web(e));            │
│ RuntimeError::TerminalUnavailable(e.to_string()))      │                      │ prepare_agent_web_pane already invents a local unavailable closure for exactly this          │
├────────────────────────────────────────────────────────┼──────────────────────┼──────────────────────────────────────────────────────────────────────────────────────────────┤│ registry.provider_of(&name).await.1                    │ 5               ine_provider_of(name), or push registry resolution down into           │
│                                                        │                      │ credentials/agents so commands pass &registry                                                │
├────────────────────────────────────────────────────────┼──────────────────────┼──────────────────────────────────────────────────────────────────────────────────────────────┤
│ Ok(state.method(args).await?)                          │ ~25                  │ Leave as-is. A macro would generate them but destroys greppability and the doc comments the  │
│                                                        │                      │ frontend contract depends on. The per-domain split already makes them cheap to scan          │
└────────────────────────────────────────────────────────┴──────────────────────┴──────────────────────────────────────────────────────────────────────────────────────────────┘

Phase 6 — docs
                                                                                                                                                                                - Root AGENTS.md: the "command not found" gotcha row now points at commands
- src-tauri/AGENTS.md: module inventory gains commands/, services/, bootstrap.rs, logging.rs; "Adding a Tauri Command" steps 2–3 change; the podman_global_runner gotcha's "befoany Tauri state wiring in run()" needs re-wording for bootstrap.
- docs/features/*/02_dev.md reference lib.rs in ~50 places. Leave them — they're dated records of decisions, not live pointers. Worth stating so nobody mass-rewrites history.

Verification

Every phase is behaviour-preserving with unchanged command names and argument shapes, so the frontend is untouched and the e2e suite is a real regression net. Per phase: cargo check → cargo test → ./build e2e light. One ./build e2e (standard) at the e Phase 5, since those two are the only phases that change how state reachesa handler.

Risk concentration is Phase 5's activity::run helper — it changes control fh, which has hand-tuned retry semantics (the "already running exitsnon-zero" comment). I'd do that command last and read the podman-startup-recovery docs before touching it.
