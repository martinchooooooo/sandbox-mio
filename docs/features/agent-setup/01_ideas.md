# Agent Setup Ideas

This document covers the ideation for a higher-level "agent setup" feature that sits above the existing mise package install flow. The feature presents a curated catalogue of coding-agent tools (starting with Claude Code), installs the required tooling into the sandbox environment via `mise.toml`, and — critically — guides the user through authentication for each agent *inside the Tauri app* before the sandbox is opened. The install and auth flows are entirely new; they build on the existing `install_package` / `RuntimeManager` pipeline and the Tauri `WebviewWindow` primitive already used for the in-app terminal.

## Current State

The package install path is implemented end-to-end: `install_package` in `src-tauri/src/lib.rs` (line 65) calls `RuntimeManager::install` on `PodmanMiseRuntime` (`src-tauri/src/runtime/mod.rs` line 145), which runs `mise use <pkg>` inside the named container. The UI lives in `src/routes/projects/SandboxItem.svelte` (lines 320–351): a plain text `<input>` bound to `packageInput` where the user types the mise tool name by hand.

Nothing in the codebase concerns itself with agents as a concept: there is no agent catalogue, no structured prerequisite list, and no authentication step. The only authentication-adjacent primitive in the app is `WebviewWindow` (`@tauri-apps/api/webviewWindow`), already used by `openTerminal()` in `SandboxItem.svelte` (line 173) to open the zellij terminal in a separate window.

Relevant seams for the new feature:
- `src-tauri/src/lib.rs` — IPC command registration; new commands will be added here.
- `src-tauri/src/runtime/types.rs` — `RuntimeManager` trait and `PodmanMiseRuntime`; new auth-related `mise_exec` calls will be added here.
- `src/routes/projects/SandboxItem.svelte` — the sandbox card that will grow an "Add agent" surface.
- `src-tauri/resources/Containerfile` — base image; any new prerequisite system packages are added here at the Debian layer.

## Proposed Model

### Agent Catalogue (Static, App-Bundled)

A static list of agent descriptors is compiled into the frontend. Each descriptor names the agent, lists which mise packages to install, describes any additional steps (e.g. environment variables that must be set), and declares an `authStrategy` — the method the app will use to authenticate the user. There is no server-side catalogue fetch; the list is versioned with the app binary.

Example descriptor shape (TypeScript, not actual file path yet):

```ts
interface AgentDescriptor {
  id: string;               // e.g. "claude-code"
  label: string;            // display name
  misePackages: string[];   // e.g. ["node@lts", "claude-code@latest"]
  authStrategy: AuthStrategy;
}
```

The catalogue lives in a new file under `src/lib/agents/` (exact name to be decided in `02_dev.md`). Starting catalogue: Claude Code only. The structure supports adding more agents (Gemini CLI, Cursor, etc.) without changing the install/auth plumbing.

### Install Phase: Mise Packages in Sequence

When the user confirms an agent installation, the app calls `install_package` for each `misePackages` entry in order, re-using the existing Tauri command unchanged. Progress is tracked per-package using the `action` runner pattern already present in `SandboxItem.svelte`. The packages are installed sequentially, not in parallel, because `mise use` rewrites `mise.toml` and concurrent writes would corrupt the manifest.

This means no new Tauri command is needed for the install phase — the existing `install_package` is called once per package in the descriptor.

### Auth Phase: In-App Login Before Sandbox Opens

After all packages are installed, the auth phase runs. The app guides the user through login *inside the Tauri process* — not via the terminal — so credentials are established before the agent tool is ever started. The mechanism depends on `authStrategy`:

**`oauth-browser`**: The app opens the agent's OAuth login URL in the system browser (via `tauri-plugin-opener`) and polls for a locally-written credential file inside the sandbox workspace. When the credential file appears (written by a `mise exec` helper command), the phase completes. This is the preferred strategy for agents that support a standard OAuth device-flow or browser-redirect flow.

**`exec-interactive`**: A new `WebviewWindow` opens a dedicated "login terminal" that runs a specific command inside the sandbox container (e.g. `claude auth login`) and streams its output. The window stays open until the command exits successfully. This mirrors how `openTerminal()` opens a zellij session in `SandboxItem.svelte` (line 173), but scoped to a single auth command with no full terminal emulator — the zellij web server is not involved. This strategy is used when the agent requires a browser-based interactive OAuth dance driven by the CLI itself (e.g. Claude Code's `claude auth login` which opens a browser tab and awaits the callback).

**`token-paste`**: The app shows an in-app dialog with a link to the agent's API key page and a single text field for the user to paste their token. On confirm, the app writes the token to the appropriate config file inside the sandbox workspace via `podman exec`. This is the fallback for agents with no OAuth flow.

### Claude Code Specific Strategy

Claude Code's auth flow is `exec-interactive`: `claude auth login` opens a browser tab and waits for the user to complete OAuth, then writes credentials to `~/.claude/` inside the container. The app detects completion by watching for exit of the auth command, not by polling the filesystem. The WebviewWindow for the auth command is closed automatically on successful exit.

The prerequisite mise packages for Claude Code are:
- `node@lts` — Claude Code's package manager dependency.
- `npm:@anthropic-ai/claude-code` — Claude Code itself, installed via mise's npm backend.

After auth completes, the credentials persist inside the container's filesystem (not the bind-mounted workspace) and survive sandbox stop/start cycles. They do not survive `destroy_sandbox` (the container is deleted). This is noted as a UX gotcha.

### UI Entry Point: Agent Panel in SandboxItem

A new "Agents" section appears below the packages section in `SandboxItem.svelte` (below line 351). It shows a row for each already-installed agent (detected by scanning `mise.toml` for known packages) and an "Add agent" button that opens a picker. The picker lists available agents from the catalogue; selecting one starts the install + auth sequence.

The install + auth sequence is presented as a stepped modal or inline panel (to be decided in `02_dev.md`): step 1 shows packages being installed, step 2 shows the auth instruction or opens the auth WebviewWindow.

## Technology Choices with Tradeoffs

### Auth execution: `exec-interactive` via WebviewWindow + container exec

The chosen approach opens a separate `WebviewWindow` that loads a lightweight page in the Tauri app. That page calls a new Tauri command `run_agent_auth(name, command)` which runs a non-interactive version of the auth command inside the container (e.g. `podman exec -i <name> claude auth login`) and streams stdout/stderr back via a Tauri `Channel`. The page displays the streamed output (URL to visit, instructions) and watches for success/failure.

Alternatives evaluated:

- **Full zellij web terminal** — open the auth command in the same zellij-backed WebviewWindow used by the Terminal button. Rejected because zellij requires the web server to be started, the forwarding port to be allocated, and the user to manually paste a one-time token — all overhead that exists to support a general-purpose multi-pane terminal, not a single-command auth flow. The auth window would look identical to the normal terminal, confusing the user about what they are supposed to do.
- **Polling a credential file in the workspace** — after instructing the user to open the terminal and run the auth command themselves, poll the workspace for the credential file. Rejected because it defeats the goal of guiding the user; the user has to context-switch to the terminal, remember the command, and the app gives no feedback on progress.
- **Native OS shell `exec`** — run `podman exec <name> claude auth login` in a bare `std::process::Command` on the Rust side, with stdout piped back to the frontend. Rejected because on Windows a browser tab must be opened and the OAuth callback must land somewhere — the auth command itself opens the browser and waits, so blocking the Tauri async executor is risky. The `Channel`-based streaming approach avoids blocking.

### Credential persistence: container filesystem (not workspace bind-mount)

Credentials written to `~/.claude/` inside the container live in the container's overlay filesystem, which persists across `stop`/`start` but is destroyed by `destroy_sandbox`. The alternative of writing credentials to the workspace bind-mount (e.g. `<workspace>/.mio/credentials/claude/`) was considered; it would survive destroy/recreate. Rejected because:
- Credential files in the workspace path are visible in the host filesystem and could be accidentally committed to git if the user opens the workspace in an editor.
- Each agent has its own credential location convention; writing them to a unified `.mio/credentials/` directory requires the auth command to be told a non-default path, which may not be supported by all agents.

The implication (credentials lost on destroy) is documented in the UI as a warning in the Agents panel.

### Agent detection: scan `mise.toml` for known packages

Whether an agent is "installed" is determined by checking if all `misePackages` in the descriptor appear in the `mise.toml` `[tools]` table (read by the existing `list_packages` / `RuntimeManager::list`). No new Tauri command is needed for detection; the frontend can compute it from the already-loaded `packages` array. The alternative of storing a separate `agents.toml` or similar file in the workspace was considered. Rejected because it introduces a second source of truth that can diverge if the user installs packages manually or via the plain package input.

### Catalogue definition: static frontend TypeScript object

The agent catalogue is a TypeScript `const` array exported from a new `src/lib/agents/catalogue.ts` file. The alternative of driving the catalogue from a Rust-side struct (serialized to the frontend) was considered. Rejected because the catalogue has no runtime-variable fields — it is purely descriptive metadata used by the frontend to construct IPC calls. Putting it in TypeScript avoids a round-trip and keeps the data co-located with the UI that renders it.

## Validation

Spike run on 2026-06-28 against the `dfsdf` sandbox container on `podman-machine-default`. Claude Code v2.1.195 was installed via `mise use node@lts` then `mise use "npm:@anthropic-ai/claude-code@latest"`, followed by a manual run of the postinstall script (see Gotchas). `claude auth login` was then invoked via `podman exec` with stdin connected to a FIFO, and the full OAuth flow was completed.

Cleared risks:

- **TTY requirement** — confirmed none needed. `claude auth login` runs correctly with piped stdin (no `-t` flag). Stdout is line-buffered without a TTY: both the browser URL and the "Paste code here if prompted >" prompt appeared in the captured pipe output. A FIFO-based stdin injection (write the code from a second `podman exec`) delivered the code to the waiting process successfully. Command: `podman exec --workdir /root/work dfsdf sh /tmp/auth_via_fifo.sh` with `printf "CODE\n" > /tmp/claude_auth_input` from a second exec.

- **OAuth callback location** — confirmed entirely server-side. `redirect_uri` is `https://platform.claude.com/oauth/code/callback` — the OAuth dance completes on claude.com, which displays the authorization code to the user on the browser page. No localhost port is opened inside the container; no published port is required at `create` time. The container needs only outbound HTTPS access (already present).

- **Auth flow shape** — confirmed as a PKCE code-exchange flow: (1) `claude auth login` prints a URL and waits at stdin; (2) user visits URL in browser, completes OAuth on claude.com; (3) platform.claude.com shows a code string of the form `<auth_code>#<state>`; (4) user pastes code into the waiting prompt; (5) CLI exchanges code + PKCE verifier with the server; (6) credentials written and process exits 0.

- **Credential paths** — confirmed two files written under `/root/`:
  - `/root/.claude.json` — main config (account UUID, email, organization, feature flags, cached GrowthBook config). Contains `oauthAccount` with `accountUuid`, `emailAddress`, `organizationUuid`, `billingType`.
  - `/root/.claude/.credentials.json` — OAuth tokens: `{ claudeAiOauth: { accessToken, refreshToken, expiresAt, scopes, subscriptionType } }`. The `expiresAt` field is a Unix epoch millisecond timestamp, enabling the app to detect expiry without a live API call.

- **Mise postinstall skip** — confirmed as a blocker. Mise installs npm packages with `--ignore-scripts=true` by default, skipping the `postinstall` script that downloads the native Claude Code binary. Running `claude --version` after a plain `mise use` fails with "native binary not installed". The postinstall must be run explicitly: `mise exec node@lts -- node <mise-install-dir>/lib/node_modules/@anthropic-ai/claude-code/install.cjs`. The install path is obtained via `mise where "npm:@anthropic-ai/claude-code"`. This step must be added as a distinct phase after the npm package install in the agent setup flow.

## Open Questions

- **Scope of the "Agents" UI panel**: Should the agent panel be a distinct section within each `SandboxItem` card (below the packages section), or a separate top-level route/page that applies agents across all sandboxes? A per-card panel follows the existing UX model but may feel repetitive if the user has many sandboxes.
- **Re-auth flow**: When credentials expire (detectable via `expiresAt` in `/root/.claude/.credentials.json`), how does the user re-authenticate? The "Agents" panel could show an auth status (authenticated / expires in N days) and offer a "Re-authenticate" button that re-runs the auth phase.
- **Additional agents beyond Claude Code**: Which other agents (Gemini CLI, GitHub Copilot CLI, Cursor) should be in the launch catalogue, and which `authStrategy` applies to each? Deciding the full v1 list before the dev plan avoids mid-implementation catalogue changes.
- **Auth window UX**: Should the auth WebviewWindow be a fixed-size modal floating over the main window, a full separate window (like the terminal), or an inline panel in the SandboxItem card? The code-paste step requires a text input and a submit button; the window must stay open until the user pastes the code and the process exits.
- **Mise `npm_args` config for postinstall**: Whether to set `npm_args = "--ignore-scripts=false"` globally in the workspace `mise.toml` (enabling postinstall for all npm packages) or only run the postinstall as an explicit step in the agent descriptor is undecided. The global opt-in is simpler but runs arbitrary postinstall scripts for all future npm packages installed by the user.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `claude` binary missing after `mise use "npm:@anthropic-ai/claude-code"` | Mise installs npm packages with `--ignore-scripts=true`, skipping the postinstall that downloads the native binary | After the npm install, run explicitly: `mise exec node@lts -- node $(mise where "npm:@anthropic-ai/claude-code")/lib/node_modules/@anthropic-ai/claude-code/install.cjs` |
| `claude auth login` appears to hang with no output | Without a pipe on stdin the process blocks at "Paste code here if prompted >" and never receives input (stdin is /dev/null in a background exec) | Always connect stdin via `Stdio::piped()` in the Rust `Command`; use a channel to write the code to stdin when the user submits it |
| Credentials lost after `destroy_sandbox` | Agent credentials in `/root/.claude/` live in the container overlay filesystem, deleted on destroy | Warn the user in the Agents panel; detect expiry from `expiresAt` in `.credentials.json` and offer a re-auth button |
| OAuth callback appears to need a container port | Not applicable for Claude Code — the callback goes to `platform.claude.com` (server-side); the container needs only outbound HTTPS | For other agents using a localhost callback, a published port would need to be added at `create` time |
| Installing packages in parallel corrupts `mise.toml` | `mise use` rewrites the entire `[tools]` table on every call | Always run prerequisite package installs sequentially |
| Catalogue version diverges from installed mise binary | If the catalogue pins `npm:@anthropic-ai/claude-code@latest` and a new version changes the auth flow, all users on next install are affected | Pin a specific version in `misePackages` and bump it deliberately on each release |
