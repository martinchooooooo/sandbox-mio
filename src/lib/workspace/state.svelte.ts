import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// Module-scoped store (mirrors src/lib/activity/activity.svelte.ts): the single `workspace://state`
// subscription for the whole global-panel document, shared by +layout.svelte (collapse/rail chrome)
// and the projects list (per-sandbox open/closed state), so neither maintains its own listener.

export type AppKind = "terminal" | "dashboard";
export type AppPane = { label: string; sandbox: string; forward_name: string; kind: AppKind; url: string };
type WorkspaceState = { apps: AppPane[]; active_app: string | null; collapsed: boolean };

let apps = $state<AppPane[]>([]);
let collapsed = $state(false);
let activeApp = $state<string | null>(null);
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

/** True if sandbox `name` currently has any embedded pane (Terminal and/or Dashboard) open. */
export function isSandboxOpen(name: string): boolean {
    return apps.some((p) => p.sandbox === name);
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
