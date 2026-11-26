import { Channel, invoke } from "@tauri-apps/api/core";
import type { Project } from "$lib/sandbox";
import type { ProviderId } from "$lib/providers";
import type { ResourceSnapshot } from "$lib/sandbox/resources.svelte";
import type { ResourceHistory } from "$lib/settings/usage";

/** Arguments of `create_sandbox`; `egress` and `starterHosts` are sent only when the provider supports an egress policy. */
export interface CreateSandboxArgs {
    name: string;
    existingPath?: string;
    /** Omitted means the backend resolves the default provider. */
    provider?: ProviderId;
    egress?: "open" | "allowlist";
    starterHosts?: string[];
}

export function createSandbox(args: CreateSandboxArgs): Promise<Project> {
    return invoke<Project>("create_sandbox", { ...args });
}

export function listSandboxes(): Promise<Project[]> {
    return invoke<Project[]>("list_sandboxes");
}

export function startSandbox(name: string): Promise<void> {
    return invoke("start_sandbox", { name });
}

export function stopSandbox(name: string): Promise<void> {
    return invoke("stop_sandbox", { name });
}

/** Rejects with kind `UnsyncedChanges` when a deployed sandbox holds unsynced work and `force` is false. */
export function destroySandbox(name: string, force: boolean): Promise<void> {
    return invoke("destroy_sandbox", { name, force });
}

export function openSandboxFolder(name: string): Promise<void> {
    return invoke("open_sandbox_folder", { name });
}

/** Starts the container event stream; every event is a raw engine event object. */
export function listenEvents(onEvent: Channel<Record<string, unknown>>): Promise<void> {
    return invoke("listen_events", { onEvent });
}

export function getResourceSnapshot(): Promise<ResourceSnapshot> {
    return invoke<ResourceSnapshot>("get_resource_snapshot");
}

export function getResourceHistory(window: "day" | "week"): Promise<ResourceHistory> {
    return invoke<ResourceHistory>("get_resource_history", { window });
}
