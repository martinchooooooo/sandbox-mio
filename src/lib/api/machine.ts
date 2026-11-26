import { Channel, invoke } from "@tauri-apps/api/core";
import type { MachineReadiness, MachineSetupEvent, ProviderId, SetupStep } from "$lib/machine/types";

export function checkPodmanReady(): Promise<void> {
    return invoke("check_podman_ready");
}

export function machineNeedsInit(): Promise<boolean> {
    return invoke<boolean>("machine_needs_init");
}

export function startPodmanMachine(): Promise<void> {
    return invoke("start_podman_machine");
}

export function provisionPodman(): Promise<void> {
    return invoke("provision_podman");
}

export function resetStuckPodmanMachine(): Promise<void> {
    return invoke("reset_stuck_podman_machine");
}

export function machineSetupSteps(provider: ProviderId): Promise<SetupStep[]> {
    return invoke<SetupStep[]>("machine_setup_steps", { provider });
}

export function machineReadiness(provider: ProviderId): Promise<MachineReadiness> {
    return invoke<MachineReadiness>("machine_readiness", { provider });
}

export function prepareMachine(provider: ProviderId, onProgress: Channel<MachineSetupEvent>): Promise<void> {
    return invoke("prepare_machine", { provider, onProgress });
}
