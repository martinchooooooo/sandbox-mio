import { createHash } from "node:crypto";

import { podman, LOCAL_CONNECTION } from "./podman-connection.js";

/** Mirrors `SESSION_NAME_MAX`/`SESSION_NAME_PREFIX` in `src-tauri/src/terminal/mod.rs`. */
const SESSION_NAME_MAX = 61;
const SESSION_NAME_PREFIX = 52;

/**
 * The Herdr session name the app derives for a sandbox: the name itself when it fits the socket
 * path, else the first 52 chars, a hyphen and 8 hex chars of the name's SHA-256. Recomputed here so
 * a change to the Rust rule breaks a spec on purpose.
 */
export function expectedSessionName(sandbox: string): string {
  if (sandbox.length <= SESSION_NAME_MAX) return sandbox;
  const hex = createHash("sha256").update(sandbox).digest("hex").slice(0, 8);
  return `${sandbox.slice(0, SESSION_NAME_PREFIX)}-${hex}`;
}

/**
 * The running Herdr sessions inside the sandbox, minus the always-listed `default` one. Exits
 * non-zero when `herdr` is absent, hence `check: false`.
 */
export function liveHerdrSessions(name: string): string[] {
  const out = podman(LOCAL_CONNECTION, ["exec", name, "herdr", "session", "list", "--json"], {
    check: false,
  });
  try {
    const parsed = JSON.parse(out) as { sessions: { name: string; running: boolean }[] };
    return parsed.sessions.filter((s) => s.running && s.name !== "default").map((s) => s.name);
  } catch {
    return [];
  }
}

/** The native terminal's view of sandbox `name` (src/lib/terminal/TerminalView.svelte). */
export function terminalView(name: string) {
  return $(`[data-terminal='${name}']`);
}

/**
 * Proves the native pane's SSH session attached and the sandbox's Herdr session runs
 * (docs/features/native-terminal/02_dev.md, Resolved Decision 7). `session` defaults to the
 * sandbox name, which is the session name for names within the socket-path limit.
 */
export async function waitForTerminalAttached(name: string, session: string = name): Promise<void> {
  await browser.waitUntil(
    async () => {
      const view = await terminalView(name);
      if (!(await view.isExisting())) return false;
      return (await view.getAttribute("data-status")) === "attached" && liveHerdrSessions(name).includes(session);
    },
    {
      timeout: 60000,
      interval: 1000,
      timeoutMsg: `the terminal of "${name}" never attached to a live Herdr session "${session}" after open_terminal_pane`,
    },
  );
}
