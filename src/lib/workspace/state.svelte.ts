import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// Module-scoped store; owns the single `workspace://state` subscription shared by the shell.

export type AppKind = "terminal" | "dashboard" | "agentweb" | "agentchat";
/** Mirrors Rust `AppPane`; a native pane (`agentchat`) has no forward and no URL. */
export type AppPane = {
    label: string;
    sandbox: string;
    workspace_path: string;
    forward_name: string | null;
    kind: AppKind;
    url: string | null;
};
/** Mirrors Rust `WorkspaceState`; `collapsed` is the app sidebar's rail state. */
type WorkspaceState = {
    apps: AppPane[];
    active_app: string | null;
    collapsed: boolean;
    sandbox_last_pane?: Record<string, string>;
};

let apps = $state<AppPane[]>([]);
let collapsed = $state(false);
let activeApp = $state<string | null>(null);
let lastPane = $state<Record<string, string>>({});
// The sandbox whose Manage dashboard shows when no pane is active (client-side only). Visibility
// must stay derived from Rust's `active_app`, never set optimistically before an invoke resolves
// (docs/features/consolidate-app-container/02_dev.md, Resolved Decision 2).
let activeSandbox = $state<string | null>(null);
// Sandboxes whose last view was Manage; Rust's `sandbox_last_pane` cannot say so, because hiding a
// pane leaves it untouched (docs/features/unified-sidebar-nav/02_dev.md, Resolved Decisions, restore target).
let manageLast = $state<Record<string, true>>({});
let started = false;

function applyState(s: WorkspaceState) {
    const previous = activeApp;
    apps = s.apps;
    collapsed = s.collapsed;
    activeApp = s.active_app;
    lastPane = s.sandbox_last_pane ?? {};
    // Only a transition moves the view: re-applying on every event would race a page click
    // (docs/features/unified-sidebar-nav/02_dev.md, Resolved Decision 10).
    if (s.active_app !== null && s.active_app !== previous) {
        const pane = s.apps.find((p) => p.label === s.active_app);
        if (pane) {
            activeSandbox = pane.sandbox;
            delete manageLast[pane.sandbox];
        }
    }
}

export function workspaceApps(): AppPane[] {
    return apps;
}

export function isCollapsed(): boolean {
    return collapsed;
}

export function activeAppLabel(): string | null {
    return activeApp;
}

/** The pane Rust reports as active, or null while the Manage dashboard or a page shows. */
export function activePane(): AppPane | null {
    return apps.find((p) => p.label === activeApp) ?? null;
}

/** The sandbox shown in the content area, or null when a routed page shows. */
export function viewSandboxName(): string | null {
    return activePane()?.sandbox ?? activeSandbox;
}

/** Open panes of sandbox `name`, in the order Rust opened them. */
export function panesOf(name: string): AppPane[] {
    return apps.filter((p) => p.sandbox === name);
}

/** True if sandbox `name` currently has a pane of `kind` open, for the launcher's open-vs-switch state. */
export function sandboxHasPane(name: string, kind: AppKind): boolean {
    return apps.some((p) => p.sandbox === name && p.kind === kind);
}

/** The pane to restore for `name`, or null when its last view was Manage or the pane is closed. */
export function restoreTarget(name: string): AppPane | null {
    if (manageLast[name]) return null;
    const label = lastPane[name];
    return apps.find((p) => p.label === label) ?? null;
}

export function activeSandboxName(): string | null {
    return activeSandbox;
}

/** Records which sandbox's Manage dashboard shows when no pane is active; no Rust state changes. */
export function setActiveSandbox(name: string): void {
    activeSandbox = name;
}

/** Records that Manage is the view to restore for `name`. */
export function markManageView(name: string): void {
    manageLast[name] = true;
}

/** Drops the Manage memory of a closed sandbox. */
export function forgetManageView(name: string): void {
    delete manageLast[name];
}

/** Takes the content area back from the sandbox view, so the routed page can show. */
export function clearActiveSandbox(): void {
    activeSandbox = null;
}

// Hydrates and keeps the store live; idempotent, safe to call in any context.
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
