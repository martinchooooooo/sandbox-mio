import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// Module-scoped store; owns the single `workspace://state` subscription shared by layout and projects list.

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
// Which Dashboard to show when no pane overlay is active (client-side only). Visibility must
// stay derived from Rust's `active_app`, never set optimistically before an invoke resolves
// (docs/features/consolidate-app-container/02_dev.md, Resolved Decision 2).
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

// True if sandbox has an open pane or its Dashboard has been viewed.
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

// Records which Dashboard should show when no overlay is active (does not invoke Rust state changes).
export function setActiveSandbox(name: string): void {
    activeSandbox = name;
}

// Clears the Dashboard-bound sandbox to avoid keeping it reported as open.
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
