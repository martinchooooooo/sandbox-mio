import { invoke } from "@tauri-apps/api/core";
import type { ForwardInfo } from "$lib/network/state.svelte";

export function listOpenForwards(): Promise<ForwardInfo[]> {
    return invoke<ForwardInfo[]>("list_open_forwards");
}

/** Returns the host loopback port the container port is exposed on. */
export function exposePort(name: string, workspacePath: string, targetPort: number): Promise<number> {
    return invoke<number>("expose_port", { name, workspacePath, targetPort });
}

export function unexposePort(name: string, targetPort: number): Promise<void> {
    return invoke("unexpose_port", { name, targetPort });
}
