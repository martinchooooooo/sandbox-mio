import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// Module-scoped store (mirrors src/lib/activity/activity.svelte.ts): the single `workspace://state`
// subscription for the whole NavigationPanel document, shared by +layout.svelte (collapse/rail chrome)
// and the projects list (per-sandbox open/closed state), so neither maintains its own listener.

export type AppKind = "terminal" | "dashboard" | "agentweb";
export type AppPane = {
    label: string;
    sandbox: string;
    workspace_path: string;
    forward_name: string;
    kind: AppKind;
    url: string;
};
type WorkspaceState = { apps: AppPane[]; active_app: string | null; collapsed: boolean } /* collapsed = NavigationPanel is collapsed */;

let apps = $state<AppPane[]>([]);
let collapsed = $state(false);
let activeApp = $state<string | null>(null);
// Which sandbox's Dashboard to show once no Terminal/AgentWeb overlay is active, purely
// client-side (not mirrored from Rust): Dashboard has no native pane of its own any more, so its
// visibility is *derived* — `activeApp === null && activeSandbox === name` — rather than tracked as
// its own "current view" flag. This must stay derived, not set optimistically before an `invoke`
// resolves: an early version set an explicit `activeView` eagerly on the Terminal/AgentWeb launch
// click, which hid the Dashboard immediately and left nothing shown if the pane never actually
// embedded (slow or failing `open_terminal_pane`/`open_agent_web`) — the fix is to let Rust's own
// `active_app` be the single source of truth for whether an overlay is showing
// (docs/features/consolidate-app-container/02_dev.md Resolved Decision 2).
let activeSandbox = $state<string | null>(null);
let started = false;

function applyState(s: WorkspaceState) {
    apps = s.apps;
    collapsed = s.collapsed;
    activeApp = s.active_app;
}

export function workspaceApps(): AppPane[] {
    return apps;
}

export function isCollapsed(): boolean {
    return collapsed;
}

export function hasTabs(): boolean {
    return apps.length > 0;
}

export function activeAppLabel(): string | null {
    return activeApp;
}

/** True if sandbox `name` counts as having an open workspace: an open Terminal/AgentWeb pane, or
 * the user has navigated to its Dashboard. There is no Rust-tracked notion of "open" for a sandbox
 * that has only ever been viewed via Dashboard (Resolved Decision 2). */
export function isSandboxOpen(name: string): boolean {
    return apps.some((p) => p.sandbox === name) || activeSandbox === name;
}

/** True if sandbox `name` currently has a pane of `kind` open, for the launcher's open-vs-switch state. */
export function sandboxHasPane(name: string, kind: AppKind): boolean {
    return apps.some((p) => p.sandbox === name && p.kind === kind);
}

export function activeSandboxName(): string | null {
    return activeSandbox;
}

/** Record `name` as the sandbox whose Dashboard should show once no Terminal/AgentWeb overlay is
 * active. Does not itself touch Rust state or hide/show anything — it only affects the derived
 * Dashboard-visibility rule above; callers switching *to* a Terminal/AgentWeb pane still `invoke`
 * `switch_pane`/`open_terminal_pane`/`open_agent_web` to do that. */
export function setActiveSandbox(name: string): void {
    activeSandbox = name;
}

/**
 * Hydrate the store and keep it live. Idempotent (guarded by `started`), safe to call from any
 * webview or plain-browser dev context: a failed subscribe/hydrate (no Tauri runtime) is caught
 * and resets the guard so a later caller can retry.
 */
export async function ensureWorkspaceState(): Promise<void> {
    if (started) return;
    started = true;
    try {
        await listen<WorkspaceState>("workspace://state", (e) => applyState(e.payload));
        applyState(await invoke<WorkspaceState>("workspace_state"));
    } catch {
        started = false;
    }
}
