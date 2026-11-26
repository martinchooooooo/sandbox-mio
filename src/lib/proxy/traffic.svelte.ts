import { unwatchProxyFlows, watchProxyFlows } from "$lib/api/proxy";
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
    /** The iron-proxy transform that refused the request. */
    rejectedBy: string | null;
    /** An `open`-mode request an allowlist policy would have refused. */
    wouldBlock: boolean;
};

// Must equal FLOW_LOG_MAX_LINES in src-tauri/src/proxy/flowlog.rs.
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

/** Starts or joins the backend poll task feeding `proxy://flows-changed`; each call is released by one `unwatchFlows`. */
export async function watchFlows(sandbox: string): Promise<void> {
    await watchProxyFlows(sandbox);
}

/** Releases one `watchFlows`; the backend stops polling with the last one, and the store keeps its rows. */
export async function unwatchFlows(sandbox: string): Promise<void> {
    await unwatchProxyFlows(sandbox);
}
