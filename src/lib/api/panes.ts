import { invoke } from "@tauri-apps/api/core";
import type { AppKind, WorkspaceState } from "$lib/workspace/state.svelte";

export function workspaceState(): Promise<WorkspaceState> {
    return invoke<WorkspaceState>("workspace_state");
}

export function watchWorkspacePorts(name: string, workspacePath: string): Promise<void> {
    return invoke("watch_workspace_ports", { name, workspacePath });
}

export function openTerminalPane(name: string, workspacePath: string): Promise<void> {
    return invoke("open_terminal_pane", { name, workspacePath });
}

export function openAgentWeb(name: string, workspacePath: string): Promise<void> {
    return invoke("open_agent_web", { name, workspacePath });
}

export function openAgentChat(name: string, workspacePath: string): Promise<void> {
    return invoke("open_agent_chat", { name, workspacePath });
}

export function prewarmAgentWeb(name: string): Promise<void> {
    return invoke("prewarm_agent_web", { name });
}

export function switchPane(name: string, kind: AppKind): Promise<void> {
    return invoke("switch_pane", { name, kind });
}

export function hideActivePane(): Promise<void> {
    return invoke("hide_active_pane");
}

export function setPanelCollapsed(collapsed: boolean): Promise<void> {
    return invoke("set_panel_collapsed", { collapsed });
}

export function closePane(name: string, kind: AppKind): Promise<void> {
    return invoke("close_pane", { name, kind });
}

export function closeWorkspace(name: string): Promise<void> {
    return invoke("close_workspace", { name });
}
