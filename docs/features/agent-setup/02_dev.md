# Agent Setup Build Plan

Build plan for the agent setup feature described in `01_ideas.md`. All spike open questions resolved below. No phases are implemented yet — this document is the starting point for implementation.

## Concrete Model

Agents panel added as new section inside each `SandboxItem` card, below the packages section. A static TypeScript catalogue (`src/lib/agents/catalogue.ts`) drives everything: it names the mise packages, postinstall steps, and auth strategy for each agent. The frontend calls existing `install_package` per mise package, then a new `install_agent_postinstall` command per postinstall step. Auth runs in a dedicated `WebviewWindow` at route `/agent-auth` that streams stdout from a piped `podman exec` process, shows the OAuth URL, accepts a code paste, and writes the code to the process stdin via a separate `send_agent_auth_input` command. Auth session state (stdin handles keyed by UUID) lives in a new Tauri state entry.

Detection of "agent installed" is purely derived from the already-loaded `packages` array in `SandboxItem`: an agent is installed if all its `misePackages` appear in that array. No new Tauri command is needed for detection. Auth status (authenticated / expired) is read from the agent's credential file inside the container via a new `get_agent_auth_status` command.

## Resolved Decisions

**Agents UI panel scope**: Per-card section inside `SandboxItem`, below the packages section. A top-level page would disconnect the agent from its sandbox context; the per-card model is consistent with how packages are managed.

**Auth window UX**: Separate `WebviewWindow` at route `/agent-auth?sandbox=<name>&agent=<id>&session=<uuid>`. The code-paste step requires a text input, and the streaming output needs visible space; an inline panel in the card is too cramped. The window auto-closes 2 seconds after the process exits 0.

**Re-auth flow**: `get_agent_auth_status` reads `expiresAt` from the credential file. The Agents panel shows "authenticated (expires in N days)" or "expired"; a "Re-authenticate" button re-opens the auth window.

**Additional agents for v1**: Claude Code only. The catalogue and postinstall infrastructure are designed to accept more entries; each new agent adds one descriptor object and (if new) a new `PostInstallStep` kind.

**Mise `npm_args` for postinstall**: Explicit `npm-postinstall` step in the agent descriptor, not a global `--ignore-scripts=false` in `mise.toml`. The global option runs arbitrary lifecycle scripts for all npm packages the user ever installs, which is a supply-chain risk.

**TTY for `claude auth login`**: None required — spike confirmed piped stdin is sufficient. The Rust spawn uses `Stdio::piped()` on both stdin and stdout.

**OAuth callback location**: Server-side (`platform.claude.com`); no container port publishing needed at `create` time.

## Remaining: Phase A — Rust Agent Module

New module `src-tauri/src/agents/` with two files. Wired into `src-tauri/src/lib.rs` via `mod agents`.

### `src-tauri/src/agents/types.rs`

```rust
// Agent-specific error type; converts into AppError via From.
pub enum AgentError {
    UnknownAgent(String),
    InstallFailed(String),
    PostInstallFailed(String),
    AuthFailed(String),
    AuthSessionNotFound(String),
    CredentialReadFailed(String),
}

// Streamed from install_agent to the frontend Channel.
#[derive(serde::Serialize, Clone)]
#[serde(tag = "kind")]
pub enum AgentInstallEvent {
    PackageStart { package: String },
    PackageDone { package: String },
    PostInstallStart { step: String },
    PostInstallDone { step: String },
    Done,
}

// Streamed from start_agent_auth to the frontend Channel.
#[derive(serde::Serialize, Clone)]
#[serde(tag = "kind")]
pub enum AgentAuthEvent {
    Output { line: String },
    Exit { code: i32 },
}

// Returned by get_agent_auth_status.
#[derive(serde::Serialize)]
pub struct AgentAuthStatus {
    pub authenticated: bool,
    pub expires_at_ms: Option<u64>,
    pub account_email: Option<String>,
}

// Holds the stdin handle for a running auth process. Stored in Tauri state.
pub struct AuthSession {
    pub stdin: tokio::process::ChildStdin,
}

// Tauri state type: session_id UUID -> AuthSession.
pub type AuthSessionMap = std::sync::Arc<tokio::sync::Mutex<std::collections::HashMap<String, AuthSession>>>;
```

Add `AgentError` to `AppError` in `src-tauri/src/error.rs` via `#[from]` and add its `kind()` arm `"Agent"`. Add `"Agent"` to the `AppErrorKind` union in `src/lib/errors.ts`.

### `src-tauri/src/agents/mod.rs`

Functions called by Tauri commands. Not methods on a trait — agent logic is command-specific and does not belong on `RuntimeManager`.

**`pub async fn install_packages`** — for each mise package in order, calls `runtime.install(name, package)` (existing trait method, unchanged), then sends `PackageStart`/`PackageDone` events to the channel.

**`pub async fn run_postinstall_npm`** — implements the `npm-postinstall` step kind. Steps:
1. Call new `RuntimeManager::tool_dir(name, tool)` (see Phase A, RuntimeManager additions) to get the install path.
2. Construct command: `mise exec <node_tool> -- node <dir>/lib/node_modules/<npm_package>/install.cjs`.
3. Call `runtime.exec_in_container(name, &args)` (see below) and await.

**`pub async fn start_auth`** — spawns `podman exec --workdir /root/work <connection> <name> mise exec -- <auth_command>` using `tokio::process::Command` with `Stdio::piped()` on stdin and `Stdio::piped()` on stdout+stderr. Generates a UUID session ID, stores the `ChildStdin` in `AuthSessionMap`. Spawns a background `tokio::task` that reads stdout line-by-line and sends `AgentAuthEvent::Output` events; on process exit, sends `AgentAuthEvent::Exit { code }`. Returns the session ID string.

Uses `runtime.local_connection()` for the Podman connection (same as `open_terminal`).

**`pub async fn send_auth_input`** — looks up session ID in `AuthSessionMap`, writes `format!("{line}\n")` to `ChildStdin` via `AsyncWriteExt::write_all`. Removes the session from the map after writing (stdin write is one-shot for PKCE code flows).

**`pub async fn cancel_auth`** — removes the session from `AuthSessionMap`, dropping `ChildStdin` (which closes the pipe end, causing the process to receive EOF and exit).

**`pub async fn get_auth_status`** — runs `podman exec <name> cat <credential_path>` (credential path is agent-specific, passed in from the catalogue descriptor on the frontend). Parses the JSON; for Claude Code reads `claudeAiOauth.expiresAt` (u64 milliseconds) and `oauthAccount.emailAddress` from `/root/.claude.json`. Returns `AgentAuthStatus { authenticated, expires_at_ms, account_email }`. If the file is absent or unreadable returns `authenticated: false`.

### RuntimeManager additions — `src-tauri/src/runtime/types.rs`

Two new methods on the `RuntimeManager` trait:

```rust
// Returns the absolute path of the mise install directory for `tool` inside container `name`.
// Equivalent to `mise where <tool>` run inside the container.
async fn tool_dir(&self, name: &str, tool: &str) -> Result<String, RuntimeError>;

// Runs an arbitrary command inside container `name` and returns its stdout.
// Used by postinstall steps. The command must be an absolute path or on $PATH.
async fn exec_in_container(&self, name: &str, args: &[&str]) -> Result<String, RuntimeError>;
```

Both implemented in `PodmanMiseRuntime` via `mise_exec` / `container_exec` helpers already present. `exec_in_container` starts the container first (same as `mise_exec`), then runs `podman exec <name> <args>`.

## Remaining: Phase B — Tauri Commands

All commands added to `src-tauri/src/lib.rs` and registered in `invoke_handler`.

```rust
// Installs all mise packages for `agent_id` sequentially, then runs its postinstall steps.
// Streams AgentInstallEvent to `on_progress`. `agent_id` is validated against a known set.
#[tauri::command]
async fn install_agent(
    name: String,
    packages: Vec<String>,          // misePackages from catalogue
    postinstall_npm: Option<(String, String)>,  // (npm_package, node_tool) if any
    on_progress: Channel<serde_json::Value>,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
) -> Result<(), AppError>

// Starts the auth process for `agent_id` in sandbox `name`.
// Returns a session_id UUID the frontend uses for subsequent stdin calls.
#[tauri::command]
async fn start_agent_auth(
    name: String,
    auth_command: Vec<String>,      // e.g. ["claude", "auth", "login"]
    on_output: Channel<serde_json::Value>,
    sessions: State<'_, AuthSessionMap>,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
) -> Result<String, AppError>      // returns session_id

// Writes a line of text to the stdin of the auth process identified by `session_id`.
#[tauri::command]
async fn send_agent_auth_input(
    session_id: String,
    input: String,
    sessions: State<'_, AuthSessionMap>,
) -> Result<(), AppError>

// Returns the current auth status by reading the credential file from the container.
#[tauri::command]
async fn get_agent_auth_status(
    name: String,
    credential_path: String,        // e.g. "/root/.claude/.credentials.json"
    expires_at_json_key: String,    // e.g. "claudeAiOauth.expiresAt"
    account_source_path: Option<String>,   // optional second file for email
    account_email_json_key: Option<String>,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
) -> Result<AgentAuthStatus, AppError>

// Cancels a running auth session (closes stdin pipe, causing process EOF).
#[tauri::command]
async fn cancel_agent_auth(
    session_id: String,
    sessions: State<'_, AuthSessionMap>,
) -> Result<(), AppError>
```

`AuthSessionMap` managed in `app.setup()` alongside `ForwardPortMap`:
```rust
app.manage(agents::types::AuthSessionMap::default());
```

## Remaining: Phase C — Frontend Catalogue

### `src/lib/agents/types.ts`

```ts
export interface NpmPostInstallStep {
  kind: "npm-postinstall";
  package: string;      // e.g. "@anthropic-ai/claude-code"
  nodeVersion: string;  // mise tool spec for node, e.g. "node@lts"
}

export type PostInstallStep = NpmPostInstallStep;
// New step kinds added here as new agents require them.

export interface ExecInteractiveAuth {
  kind: "exec-interactive";
  command: string[];              // e.g. ["claude", "auth", "login"]
  credentialFile: string;         // container path to check for auth status
  expiresAtJsonKey: string;       // dot-path into the JSON, e.g. "claudeAiOauth.expiresAt"
  accountSourceFile?: string;     // optional second file for human-readable account info
  accountEmailJsonKey?: string;   // dot-path for email in accountSourceFile
}

export type AuthStrategy = ExecInteractiveAuth;
// TokenPaste and OAuthBrowser added here when a new agent needs them.

export interface AgentDescriptor {
  id: string;
  label: string;
  description: string;
  misePackages: string[];
  postInstallSteps: PostInstallStep[];
  auth: AuthStrategy;
}
```

### `src/lib/agents/catalogue.ts`

```ts
import type { AgentDescriptor } from "./types";

export const AGENTS: AgentDescriptor[] = [
  {
    id: "claude-code",
    label: "Claude Code",
    description: "Anthropic's AI coding agent",
    misePackages: [
      "node@lts",
      "npm:@anthropic-ai/claude-code@2.1.195",
    ],
    postInstallSteps: [
      {
        kind: "npm-postinstall",
        package: "@anthropic-ai/claude-code",
        nodeVersion: "node@lts",
      },
    ],
    auth: {
      kind: "exec-interactive",
      command: ["claude", "auth", "login"],
      credentialFile: "/root/.claude/.credentials.json",
      expiresAtJsonKey: "claudeAiOauth.expiresAt",
      accountSourceFile: "/root/.claude.json",
      accountEmailJsonKey: "oauthAccount.emailAddress",
    },
  },
];
```

## Remaining: Phase D — Auth WebviewWindow Route

### `src/routes/agent-auth/+page.svelte`

Loaded in a `WebviewWindow` opened from `SandboxItem`. URL query params: `?sandbox=<name>&agent=<id>&session=<uuid>` (session is pre-allocated by the opener before creating the window — see Phase E).

Wait: the session ID is returned by `start_agent_auth`, which must be called before the window opens. The opener (`SandboxItem`) calls `start_agent_auth` first (obtaining the session ID and starting the stream), then opens the `WebviewWindow` at a URL that includes `session=<id>`. The `on_output` channel is bound to a Tauri event name derived from the session ID: `agent-auth-<session_id>`. The auth page listens to that event via `listen("agent-auth-<session_id>", ...)`.

Page behaviour:
1. On mount, reads `sandbox`, `agent`, `session` from `$page.url.searchParams`. Looks up the descriptor from `AGENTS` by agent ID.
2. Listens for `AgentAuthEvent` on the Tauri event channel.
3. Renders streamed output lines in a scrollable pre-formatted area (monospace, dark background).
4. Parses each `Output` line for a URL starting with `https://claude.com` — when found, shows a button "Open in browser" (via `@tauri-apps/plugin-opener`) and copies the URL to clipboard.
5. Parses each `Output` line for `"Paste code here"` — when found, shows a text input and a "Submit" button. On submit, calls `send_agent_auth_input(session, inputValue)`.
6. On `Exit { code: 0 }`: shows "Authentication successful." and auto-closes the window after 2 seconds via `window.close()`.
7. On `Exit { code != 0 }`: shows "Authentication failed (exit code N)" with a "Close" button.

The auth window has no navigation. Its only actions are "Open in browser" and code paste.

## Remaining: Phase E — SandboxItem Integration

Changes to `src/routes/projects/SandboxItem.svelte`.

### New state

```ts
import { AGENTS } from "$lib/agents/catalogue";
import type { AgentDescriptor } from "$lib/agents/types";

let agentStatuses = $state<Record<string, { authenticated: boolean; expiresAtMs: number | null; email: string | null }>>({});
let installingAgent = $state<string | null>(null);   // agent id currently installing
let installLog = $state<string[]>([]);               // streamed install events
let showAgentPicker = $state(false);
```

### Derived: installed agents

```ts
const installedAgents = $derived(
  AGENTS.filter(a => a.misePackages.every(p => packages.includes(p)))
);
const availableAgents = $derived(
  AGENTS.filter(a => !installedAgents.includes(a))
);
```

### `installAgent(descriptor: AgentDescriptor)`

1. Sets `installingAgent = descriptor.id`, clears `installLog`.
2. Calls `install_agent` Tauri command with `{ name, packages: descriptor.misePackages, postinstall_npm: ... }` and a `Channel` that appends events to `installLog`.
3. On success: `installingAgent = null`, calls `loadPackages()` to refresh.
4. On error: shows error via `action.error`.

### `openAgentAuth(descriptor: AgentDescriptor)`

1. Calls `start_agent_auth({ name, authCommand: descriptor.auth.command, onOutput: channel })`. The `Channel` event name is `agent-auth-<session_id>` — the channel is a Tauri channel backed by an event.
2. Receives session ID from the command.
3. Opens `WebviewWindow` with label `agent-auth-${name}-${descriptor.id}`, URL `/agent-auth?sandbox=${name}&agent=${descriptor.id}&session=${sessionId}`, fixed size ~540×460.
4. If a window with that label already exists, focuses it instead.

### `loadAgentStatuses()`

For each agent in `installedAgents`, calls `get_agent_auth_status` with the credential paths from its descriptor. Stores results in `agentStatuses`. Called in `onMount` (after `loadPackages`) and after `installAgent` succeeds.

### New Agents section (below line 351)

```svelte
<div class="flex flex-col gap-1.5 border-t pt-2">
  <div class="flex items-center justify-between">
    <span class="text-muted-foreground text-xs shrink-0">{m.projects_agents()}</span>
    {#if availableAgents.length > 0}
      <Button size="sm" variant="ghost" onclick={() => showAgentPicker = true}
        disabled={installingAgent !== null || action.busy !== null}>
        {m.projects_add_agent()}
      </Button>
    {/if}
  </div>

  {#if installedAgents.length === 0 && !showAgentPicker}
    <span class="text-muted-foreground text-xs">{m.projects_no_agents()}</span>
  {/if}

  {#each installedAgents as agent}
    {@const status = agentStatuses[agent.id]}
    <div class="flex items-center justify-between gap-2 text-xs">
      <span class="font-medium">{agent.label}</span>
      {#if status?.authenticated}
        <span class="text-muted-foreground">
          {status.email ?? "authenticated"}
        </span>
      {:else}
        <span class="text-amber-600 dark:text-amber-400">{m.projects_agent_not_authed()}</span>
      {/if}
      <Button size="sm" variant="ghost" onclick={() => openAgentAuth(agent)}
        disabled={action.busy !== null}>
        {status?.authenticated ? m.projects_agent_reauth() : m.projects_agent_auth()}
      </Button>
    </div>
  {/each}

  {#if installingAgent !== null}
    <div class="text-xs text-muted-foreground space-y-0.5">
      {#each installLog as line}
        <div>{line}</div>
      {/each}
    </div>
  {/if}

  {#if showAgentPicker}
    <div class="flex flex-col gap-1.5">
      {#each availableAgents as agent}
        <div class="flex items-center justify-between gap-2">
          <div>
            <span class="text-xs font-medium">{agent.label}</span>
            <span class="text-xs text-muted-foreground ml-1">{agent.description}</span>
          </div>
          <Button size="sm" onclick={() => { showAgentPicker = false; installAgent(agent); }}
            disabled={installingAgent !== null}>
            {m.projects_install()}
          </Button>
        </div>
      {/each}
      <Button size="sm" variant="outline" onclick={() => showAgentPicker = false}>
        {m.projects_cancel()}
      </Button>
    </div>
  {/if}
</div>
```

### Translation keys to add to `messages/en.json` (and mirror to `de.json`, `ru.json`)

```json
"projects_agents": "Agents",
"projects_add_agent": "Add agent",
"projects_no_agents": "No agents installed",
"projects_agent_auth": "Authenticate",
"projects_agent_reauth": "Re-authenticate",
"projects_agent_not_authed": "not authenticated"
```

## Lifecycle / Cross-Cutting Concerns

**Auth session cleanup**: `AuthSession` entries are removed from `AuthSessionMap` either when `send_agent_auth_input` writes the code (one-shot), when `cancel_agent_auth` is called (window closed by user), or when the background stdout-reading task sends `Exit` and drops the session. The background task must also remove the session from the map on exit to prevent stale entries.

**One auth window per agent per sandbox**: The `WebviewWindow` label `agent-auth-${name}-${descriptor.id}` is unique per sandbox + agent pair. `openAgentAuth` checks for an existing window with that label and focuses it instead of opening a duplicate, matching the `openTerminal` pattern in `SandboxItem.svelte` (line 173).

**Channel event routing**: The `start_agent_auth` command receives a Tauri `Channel<serde_json::Value>`. The channel ID is embedded in the opened WebviewWindow URL (not as a URL param — channels are not URL-serialisable). Instead: `SandboxItem` opens the auth WebviewWindow, then uses `WebviewWindow.emit("agent-auth-ready", { channelId })` to hand the channel ID to the page; the page calls `invoke("bind_agent_auth_channel", { sessionId, channelId })` which wires the existing auth stream to the channel. Alternatively, use a simpler approach: derive a stable Tauri event name from session ID (`agent-auth-output-<uuid>`) and use `appWindow.listen` in the page. The simpler approach is preferred.

**Install while terminal open**: Installing a package starts the container if stopped, same as `mise_exec`. This is the existing behaviour and unchanged.

**Credential expiry display**: `expiresAt` is a Unix millisecond timestamp. `loadAgentStatuses` compares it to `Date.now()`. If `expiresAt < Date.now()`, `authenticated` is reported as `false` even if the file exists. The refresh token may still allow silent re-auth, but the app does not attempt silent re-auth — it always shows the Re-authenticate button.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `claude` binary missing after package install | Mise skips npm postinstall with `--ignore-scripts=true` | `run_npm_postinstall` in Phase A handles this; it must run before `PackageDone` is emitted for the npm package step |
| Auth stream never sends `Exit` event | Background task panics or the channel is dropped before the process exits | The background task must catch errors and always send `AgentAuthEvent::Exit` before returning |
| Auth WebviewWindow URL contains untrusted data | `name` (sandbox name) is user-controlled and interpolated into the window URL | Sandbox names are already validated by the backend on create; URL-encode the param anyway |
| `send_agent_auth_input` called after session removed | Race: user submits code, window closes, backend removes session | Treat `AuthSessionNotFound` as non-fatal in the frontend — if the session is gone the auth already completed or was cancelled |
| New agent catalogue entry breaks if `exec_in_container` removed | `exec_in_container` is added to `RuntimeManager` for postinstall; it must not be removed as "unused" if the Claude Code agent is the only caller | Keep the method; it is the designed extension point for future agent postinstall steps |
