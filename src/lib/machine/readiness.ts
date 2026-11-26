import { Channel } from "@tauri-apps/api/core";
import {
    machineReadiness,
    machineSetupSteps,
    prepareMachine as prepareMachineCommand,
} from "$lib/api/machine";
import { parseError } from "$lib/errors";
import type { MachineReadiness, MachineSetupEvent, ProviderId, SetupStep } from "./types";

/** Provider-neutral welcome-flow readiness (`SetupProgress.svelte`), distinct from the Podman-only
 * recovery gate in `$lib/podman/setup.svelte` that the projects page and the setup store's image
 * job keep using unchanged. */
export async function fetchMachineSetupSteps(provider: ProviderId): Promise<SetupStep[]> {
    try {
        return await machineSetupSteps(provider);
    } catch {
        return [];
    }
}

export async function fetchMachineReadiness(provider: ProviderId): Promise<MachineReadiness> {
    return machineReadiness(provider);
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
        await prepareMachineCommand(provider, channel);
        return { ok: true, message: "", rebootRequired };
    } catch (e) {
        return { ok: false, message: parseError(e).message, rebootRequired };
    }
}
