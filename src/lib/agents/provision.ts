import { invoke } from "@tauri-apps/api/core";
import type { AgentDescriptor } from "./types";
import { FALLBACK_MODEL_ID } from "./local-llm";
import { buildLocalLlmProvider } from "./opencode";
import { buildPiLocalLlmProvider, PI_CONFIG_DIR_CONTAINER, PI_CONFIG_ENV } from "./pi";

/** Writes `descriptor`'s local-model config host-side, so it works whether or not the sandbox runs;
 * the only branch on `provision.kind`. `runningModelId` is null when no inference server runs. */
export async function provisionAgent(
    descriptor: AgentDescriptor,
    workspacePath: string,
    runningModelId: string | null,
): Promise<void> {
    const provision = descriptor.provision;
    if (!provision) return;
    const modelId = runningModelId ?? FALLBACK_MODEL_ID;
    switch (provision.kind) {
        case "opencode-provider": {
            const { providerId, providerJson, defaultModel } = buildLocalLlmProvider(modelId);
            await invoke("provision_opencode", {
                workspacePath,
                relativePath: provision.relativeConfigPath,
                providerId,
                providerJson,
                defaultModel,
            });
            return;
        }
        case "pi-provider": {
            const { providerId, providerJson, defaultModel } = buildPiLocalLlmProvider(modelId);
            await invoke("provision_pi", {
                workspacePath,
                relativePath: provision.relativeConfigPath,
                providerId,
                providerJson,
                defaultProvider: providerId,
                defaultModel,
                configEnv: PI_CONFIG_ENV,
                configDirContainer: PI_CONFIG_DIR_CONTAINER,
            });
            return;
        }
    }
}
