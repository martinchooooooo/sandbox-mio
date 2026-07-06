import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// Module-scoped store (mirrors src/lib/workspace/state.svelte.ts): the single
// `network://forwards-changed` subscription for the whole app. MutagenNetworkManager (and its mock)
// emit the full forwards snapshot after every mutation, so callers never need to refetch manually.

export type ServiceKind =
    | { kind: "terminal" }
    | { kind: "agent-web" }
    | { kind: "exposed"; targetPort: number };

export type ForwardInfo = {
    sandbox: string;
    kind: ServiceKind;
    hostPort: number;
    connection: string;
    remotePort: number;
    forwardName: string;
    /** Set when this exposure was declared via `.mio/ports.json` rather than the manual UI; also
     *  how `PortsManager.svelte` decides not to show a close (X) button for it. */
    label: string | null;
    /** Set when this exposure was discovered by scanning the sandbox's listening sockets rather
     *  than declared or manually exposed; like `label`, hides the close (X) button. */
    autoDetected: boolean;
};

const EVENT_NAME = "network://forwards-changed";

let forwards = $state<ForwardInfo[]>([]);
let started = false;

export function openForwards(): ForwardInfo[] {
    return forwards;
}

/** Forwards currently tracked for a given sandbox, for `PortsManager.svelte`'s per-sandbox view. */
export function openForwardsFor(sandbox: string): ForwardInfo[] {
    return forwards.filter((f) => f.sandbox === sandbox);
}

/**
 * Hydrate the store and keep it live. Idempotent (guarded by `started`), safe to call from any
 * webview or plain-browser dev context: a failed subscribe/hydrate (no Tauri runtime) is caught
 * and resets the guard so a later caller can retry.
 */
export async function ensureNetworkState(): Promise<void> {
    if (started) return;
    started = true;
    try {
        await listen<ForwardInfo[]>(EVENT_NAME, (e) => {
            forwards = e.payload;
        });
        forwards = await invoke<ForwardInfo[]>("list_open_forwards");
    } catch {
        started = false;
    }
}

/**
 * Expose `targetPort` inside sandbox `name` to a Windows-host loopback port. Returns that port.
 * `workspacePath` bootstraps `.mio/ports.json` on the Rust side if it doesn't exist yet.
 */
export async function exposePort(name: string, workspacePath: string, targetPort: number): Promise<number> {
    return invoke<number>("expose_port", { name, workspacePath, targetPort });
}

export async function unexposePort(name: string, targetPort: number): Promise<void> {
    await invoke("unexpose_port", { name, targetPort });
}
