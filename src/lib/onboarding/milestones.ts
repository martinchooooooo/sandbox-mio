import { invoke } from "@tauri-apps/api/core";

// Webview start is the best proxy for app first open the frontend can see.
const startedAt = Date.now();
const seen = new Set<string>();

/** First-run milestones, in order; a name is the log field the time-to-wow metric is read from. */
export type Milestone =
    | "heads-up-confirmed"
    | "model-downloaded"
    | "server-started"
    | "sandbox-created"
    | "pi-installed";

/** Records `name` once per app run, with the time since the webview opened. Never throws. */
export function recordMilestone(name: Milestone): void {
    if (seen.has(name)) return;
    seen.add(name);
    invoke("log_milestone", { name, elapsedMs: Date.now() - startedAt }).catch(() => {});
}
