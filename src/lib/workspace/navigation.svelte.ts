import { invoke } from "@tauri-apps/api/core";
import { goto } from "$app/navigation";
import {
    clearActiveSandbox,
    activeSandboxName,
    forgetManageView,
    markManageView,
    restoreTarget,
    setActiveSandbox,
    type AppPane,
} from "$lib/workspace/state.svelte";

// Every way the shell moves between pages and sandboxes, so components never re-derive the order
// of client-side state and pane commands. The content area itself is derived in the store
// (docs/features/unified-sidebar-nav/02_dev.md, Concrete Model).

let createFocusPending = $state(false);

/**
 * Shows a routed page. The navigation lands before the pane is hidden, so the old page never
 * flashes in between; panes keep running behind the page.
 */
export async function showPage(href: string): Promise<void> {
    await goto(href);
    clearActiveSandbox();
    try {
        await invoke("hide_active_pane");
    } catch (e) {
        console.error(e);
    }
}

/**
 * Brings sandbox `name` into view: its last view when `restore` is set (the sidebar), otherwise
 * its Manage dashboard (the Home card). Rejects when a command fails, for the caller to report.
 */
export async function openSandbox(
    name: string,
    workspacePath: string,
    { restore }: { restore: boolean },
): Promise<void> {
    const target = restore ? restoreTarget(name) : null;
    // A restored pane brings its sandbox into view through the store once Rust confirms it; setting
    // the sandbox here first would mount Manage in the meantime.
    if (!target) {
        setActiveSandbox(name);
        markManageView(name);
    }
    await Promise.all([
        invoke("watch_workspace_ports", { name, workspacePath }),
        target
            ? invoke("switch_pane", { name, kind: target.kind })
            : invoke("hide_active_pane"),
    ]);
}

/** Shows sandbox `name`'s Manage dashboard once Rust confirms no pane is active. */
export async function showManage(name: string): Promise<void> {
    setActiveSandbox(name);
    markManageView(name);
    try {
        await invoke("hide_active_pane");
    } catch (e) {
        console.error(e);
    }
}

export async function activatePane(pane: AppPane): Promise<void> {
    setActiveSandbox(pane.sandbox);
    try {
        await invoke("switch_pane", { name: pane.sandbox, kind: pane.kind });
    } catch (e) {
        console.error(e);
    }
}

export async function closePane(pane: AppPane): Promise<void> {
    try {
        await invoke("close_pane", { name: pane.sandbox, kind: pane.kind });
    } catch (e) {
        console.error(e);
    }
}

/**
 * Closes every pane of sandbox `name` and returns to Home. When Rust activates another sandbox's
 * pane as the neighbour, that sandbox stays in view instead.
 */
export async function closeSandbox(name: string): Promise<void> {
    try {
        await invoke("close_workspace", { name });
    } catch (e) {
        console.error(e);
    }
    forgetManageView(name);
    if (activeSandboxName() === name) clearActiveSandbox();
    await goto("/projects");
}

/** Opens Home with the create card's name field focused. */
export async function newSandbox(): Promise<void> {
    await showPage("/projects");
    createFocusPending = true;
}

/** Whether the create card should take focus; read once by the card, then cleared. */
export function takeCreateFocus(): boolean {
    if (!createFocusPending) return false;
    createFocusPending = false;
    return true;
}

/** Reactive read of the pending focus request, for an effect to re-run on. */
export function createFocusRequested(): boolean {
    return createFocusPending;
}
