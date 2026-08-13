import { invoke, Channel } from "@tauri-apps/api/core";
import { parseError } from "$lib/errors";
import type { MachineReadiness, MachineSetupEvent, ProviderId, SetupStep } from "./types";

/** Provider-neutral welcome-flow readiness (`CreateVmStep.svelte`), distinct from the Podman-only
 * recovery gate in `$lib/podman/setup.svelte` that the projects page and download-necessities step
 * keep using unchanged (`docs/features/microsandbox-provider/02_dev.md`, Phase I). */
export async function fetchMachineSetupSteps(provider: ProviderId): Promise<SetupStep[]> {
    try {
        return await invoke<SetupStep[]>("machine_setup_steps", { provider });
    } catch {
        return [];
    }
}

export async function fetchMachineReadiness(provider: ProviderId): Promise<MachineReadiness> {
    return invoke<MachineReadiness>("machine_readiness", { provider });
}

export interface PrepareMachineResult {
    ok: boolean;
    message: string;
    rebootRequired: boolean;
}

/** Runs `prepare_machine`, replaying every streamed event to `onEvent` as it arrives. */
export async function prepareMachine(
    provider: ProviderId,
    onEvent: (e: MachineSetupEvent) => void,
): Promise<PrepareMachineResult> {
    const channel = new Channel<MachineSetupEvent>();
    let rebootRequired = false;
    channel.onmessage = (e) => {
        if (e.kind === "rebootRequired") rebootRequired = true;
        onEvent(e);
    };
    try {
        await invoke("prepare_machine", { provider, onProgress: channel });
        return { ok: true, message: "", rebootRequired };
    } catch (e) {
        return { ok: false, message: parseError(e).message, rebootRequired };
    }
}
