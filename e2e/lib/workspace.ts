// Drives the AppContainer workspace registry (src-tauri/src/workspace/mod.rs) through its Tauri
// commands. Terminal/AgentWeb panes are the only native surface besides `main-navigation-panel`.

export type AppKind = "terminal" | "agentweb" | "agentchat";

export interface AppPane {
  label: string;
  sandbox: string;
  workspace_path: string;
  forward_name: string | null;
  kind: AppKind;
  url: string | null;
}

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface WorkspaceState {
  apps: AppPane[];
  active_app: string | null;
  collapsed: boolean;
  overlay: Rect;
  sandbox_last_pane: Record<string, string>;
}

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return browser.execute(
    (c, a) => {
      // @ts-expect-error __TAURI_INTERNALS__ is injected into the webview at runtime.
      return window.__TAURI_INTERNALS__.invoke(c, a);
    },
    cmd,
    args,
  );
}

export async function workspaceState(): Promise<WorkspaceState> {
  return invoke<WorkspaceState>("workspace_state");
}

export async function setPanelCollapsed(collapsed: boolean): Promise<void> {
  await invoke("set_panel_collapsed", { collapsed });
}

export async function switchApp(name: string): Promise<void> {
  await invoke("switch_app", { name });
}

export async function switchPane(name: string, kind: AppKind): Promise<void> {
  await invoke("switch_pane", { name, kind });
}

export async function hideActivePane(): Promise<void> {
  await invoke("hide_active_pane");
}

export async function openAgentWeb(name: string, workspacePath: string): Promise<void> {
  await invoke("open_agent_web", { name, workspacePath });
}

export async function openTerminalPane(name: string, workspacePath: string): Promise<void> {
  await invoke("open_terminal_pane", { name, workspacePath });
}

export async function closeWorkspace(name: string): Promise<void> {
  await invoke("close_workspace", { name });
}

export async function closePane(name: string, kind: AppKind): Promise<void> {
  await invoke("close_pane", { name, kind });
}

export function panesFor(state: WorkspaceState, sandbox: string): AppPane[] {
  return state.apps.filter((p) => p.sandbox === sandbox);
}

export function paneFor(state: WorkspaceState, sandbox: string, kind: AppKind): AppPane | undefined {
  return state.apps.find((p) => p.sandbox === sandbox && p.kind === kind);
}
