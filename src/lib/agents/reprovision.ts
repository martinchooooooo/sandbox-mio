import { invoke } from "@tauri-apps/api/core";
import type { Project } from "$lib/sandbox";
import { AGENTS, miseKey } from "./catalogue";
import { buildLocalLlmProvider } from "./opencode";

// Best-effort re-provision across all sandboxes; sandboxes that fail are skipped.
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
