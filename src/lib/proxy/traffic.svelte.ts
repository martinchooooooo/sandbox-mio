import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// Module-scoped store; owns the single `proxy://flows-changed` subscription.

/** Mirrors Rust `proxy::flowlog::FlowEntry` (camelCase per its own `serde(rename_all)`). */
export type FlowEntry = {
    id: number;
    method: string;
    host: string;
    path: string;
    status: number | null;
    ts: number;
    durationMs: number | null;
    reqBytes: number | null;
    respBytes: number | null;
    secretInjected: boolean;
    error: string | null;
};

// Must equal FLOW_LOG_MAX_LINES in src-tauri/src/proxy/flowlog.rs and
// src-tauri/resources/mitm_flowlog_addon.py.
const FLOW_LOG_MAX_LINES = 500;

const EVENT_NAME = "proxy://flows-changed";

// The event carries only the flows new since the last backend tick, so this store owns
// accumulation and capping (docs/features/proxy-traffic-viewer/02_dev.md, Resolved Decision 3).
type FlowsChanged = { sandbox: string; flows: FlowEntry[] };

let flows = $state<Record<string, FlowEntry[]>>({});
let started = false;

/** Flows captured for a given sandbox, oldest first. */
export function flowsFor(sandbox: string): FlowEntry[] {
    return flows[sandbox] ?? [];
}

// Keeps the store live; idempotent, safe to call in any context.
export async function ensureTrafficState(): Promise<void> {
    if (started) return;
    started = true;
    try {
        await listen<FlowsChanged>(EVENT_NAME, (e) => {
            // A re-spawned poller replays the whole log, and rows are keyed by id.
            const current = flows[e.payload.sandbox] ?? [];
            const seen = new Set(current.map((f) => f.id));
            const next = [...current, ...e.payload.flows.filter((f) => !seen.has(f.id))];
            flows = { ...flows, [e.payload.sandbox]: next.slice(-FLOW_LOG_MAX_LINES) };
        });
    } catch {
        started = false;
    }
}

/** Starts the backend poll task feeding `proxy://flows-changed`; idempotent backend-side. */
export async function watchFlows(sandbox: string): Promise<void> {
    await invoke("watch_proxy_flows", { name: sandbox });
}
