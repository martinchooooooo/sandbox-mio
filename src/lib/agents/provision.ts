import { getSettings } from "$lib/api/app";
import { provisionOpencode, provisionPi } from "$lib/api/agents";
import type { AgentDescriptor } from "./types";
import { FALLBACK_MODEL_ID } from "./local-llm";
import { buildLocalLlmProvider } from "./opencode";
import { buildPiLocalLlmProvider, DEFAULT_LLAMA_CTX_SIZE, PI_CONFIG_DIR_CONTAINER, PI_CONFIG_ENV } from "./pi";

/** The context size the configured engine serves, or `null` when it is not known (Foundry). */
async function servedContextSize(): Promise<number | null> {
    const settings = await getSettings();
    if (settings.inferenceEngine !== "llama") return null;
    return settings.llamaCtxSize ?? DEFAULT_LLAMA_CTX_SIZE;
}

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
            await provisionOpencode({
                workspacePath,
                relativePath: provision.relativeConfigPath,
                providerId,
                providerJson,
                defaultModel,
            });
            return;
        }
        case "pi-provider": {
            const { providerId, providerJson, defaultModel } = buildPiLocalLlmProvider(
                modelId,
                await servedContextSize(),
            );
            await provisionPi({
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
