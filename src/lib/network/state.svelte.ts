import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// Module-scoped store; owns the single `network://forwards-changed` subscription.

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

// Hydrates and keeps the store live; idempotent, safe to call in any context.
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

// Returns the Windows-host loopback port the container port is exposed to.
export async function exposePort(name: string, workspacePath: string, targetPort: number): Promise<number> {
    return invoke<number>("expose_port", { name, workspacePath, targetPort });
}

export async function unexposePort(name: string, targetPort: number): Promise<void> {
    await invoke("unexpose_port", { name, targetPort });
}
