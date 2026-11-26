import { Channel } from "@tauri-apps/api/core";
import { installAgent } from "$lib/api/agents";
import { inferenceStatus } from "$lib/api/inference";
import { onboardingSeed } from "./catalogue";
import { provisionAgent } from "./provision";
import type { AgentDescriptor, AgentInstallEvent } from "./types";

/** Progress notes of an install, in order: backend events plus a marker before provisioning. */
export type InstallProgress =
    | { kind: "event"; event: AgentInstallEvent }
    | { kind: "provisioning" }
    | { kind: "provisioned" };

/**
 * Installs `descriptor` into the sandbox and, for agents that declare `provision`, points them at
 * the model the inference server is serving right now. Rejects with the failing command's error.
 * Start the server before calling this, or pi is provisioned with the fallback model id.
 */
export async function installAgentInSandbox(
    sandboxName: string,
    workspacePath: string,
    descriptor: AgentDescriptor,
    onProgress?: (p: InstallProgress) => void,
): Promise<void> {
    const npmStep = descriptor.postInstallSteps.find((s) => s.kind === "npm-postinstall");
    const channel = new Channel<AgentInstallEvent>();
    channel.onmessage = (event) => onProgress?.({ kind: "event", event });
    await installAgent({
        name: sandboxName,
        packages: descriptor.misePackages,
        postinstallNpm: npmStep ? [npmStep.package, npmStep.nodeVersion] : null,
        credentialService: descriptor.credentialService ?? null,
        onboarding: onboardingSeed(descriptor),
        onProgress: channel,
    });
    if (!descriptor.provision) return;
    onProgress?.({ kind: "provisioning" });
    let runningModelId: string | null = null;
    try {
        const status = await inferenceStatus();
        runningModelId = status.running ? status.modelId : null;
    } catch {
        runningModelId = null;
    }
    await provisionAgent(descriptor, workspacePath, runningModelId);
    onProgress?.({ kind: "provisioned" });
}
