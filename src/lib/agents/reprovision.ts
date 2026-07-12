// Keeps installed agents' provider configs pointed at whichever local model is actually running.
// Without this, OpenCode's `opencode.json` default model would stay pinned to whatever was
// running the first time it was installed, so switching models on the /models page silently left
// OpenCode calling a model the server no longer serves.
import { invoke } from "@tauri-apps/api/core";
import type { Project } from "$lib/sandbox";
import { AGENTS, miseKey } from "./catalogue";
import { buildLocalLlmProvider } from "./opencode";

/** Re-provision every installed OpenCode agent, across every sandbox, to default to `modelId`.
 * Best-effort: a sandbox that fails to list packages or re-provision (e.g. stopped, unreachable)
 * is skipped rather than aborting the rest. Called after `start_inference` resolves. */
export async function reprovisionInstalledAgents(modelId: string): Promise<void> {
    const opencode = AGENTS.find((a) => a.provision?.kind === "opencode-provider");
    if (!opencode?.provision) return;

    let sandboxes: Project[];
    try {
        sandboxes = await invoke<Project[]>("list_sandboxes");
    } catch {
        return;
    }

    const { providerId, providerJson, defaultModel } = buildLocalLlmProvider(modelId);
    await Promise.all(
        sandboxes.map(async (sandbox) => {
            try {
                const packages = await invoke<string[]>("list_packages", { workspacePath: sandbox.workspace_path });
                const installed = opencode.misePackages.every((p) => packages.includes(miseKey(p)));
                if (!installed) return;
                await invoke("provision_opencode", {
                    workspacePath: sandbox.workspace_path,
                    relativePath: opencode.provision!.relativeConfigPath,
                    providerId,
                    providerJson,
                    defaultModel,
                });
            } catch {
                // Best-effort; a stopped/unreachable sandbox shouldn't block the others.
            }
        }),
    );
}
