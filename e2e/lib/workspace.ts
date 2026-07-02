// Reads/drives the single main-window workspace registry (src-tauri/src/workspace/mod.rs) through
// its Tauri commands. The container's child webviews (main-global, main-switcher, and each
// sandbox's terminal pane) are separate native surfaces a WebDriver session attached to the app
// webview never sees, so specs cannot click into them — they invoke these commands directly via
// window.__TAURI_INTERNALS__ instead (the technique e2e/AGENTS.md's light specs use for
// browser-side round-trips), then assert the returned state.

export type AppKind = "terminal";

export interface AppPane {
  label: string;
  sandbox: string;
  forward_name: string;
  kind: AppKind;
  url: string;
}

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface WorkspaceRects {
  global: Rect;
  switcher: Rect;
  content: Rect;
}

export interface WorkspaceState {
  apps: AppPane[];
  active_app: string | null;
  collapsed: boolean;
  rects: WorkspaceRects;
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

// Read-back of the whole registry: every open pane, the active one, the collapse flag, and the
// last-applied logical rects (not live physical bounds — see set_collapsed in mod.rs).
export async function workspaceState(): Promise<WorkspaceState> {
  return invoke<WorkspaceState>("workspace_state");
}

export async function setPanelCollapsed(collapsed: boolean): Promise<void> {
  await invoke("set_panel_collapsed", { collapsed });
}

export async function switchApp(name: string): Promise<void> {
  await invoke("switch_app", { name });
}

export function paneFor(state: WorkspaceState, sandbox: string): AppPane | undefined {
  return state.apps.find((p) => p.sandbox === sandbox);
}
