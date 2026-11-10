import { invoke } from "@tauri-apps/api/core";
import type { Project } from "$lib/sandbox";
import { AGENTS, miseKey } from "./catalogue";
import { provisionAgent } from "./provision";

/** Re-provisions every installed agent that declares `provision`, in every sandbox, best-effort per
 * sandbox (docs/features/local-llm-reachability/02_dev.md, Resolved Decision 11). */
export async function reprovisionInstalledAgents(runningModelId: string | null): Promise<void> {
    const agents = AGENTS.filter((a) => a.provision);
    if (agents.length === 0) return;

    let sandboxes: Project[];
    try {
        sandboxes = await invoke<Project[]>("list_sandboxes");
    } catch {
        return;
    }

    await Promise.all(
        sandboxes.map(async (sandbox) => {
            let packages: string[];
            try {
                packages = await invoke<string[]>("list_packages", { workspacePath: sandbox.workspace_path });
            } catch {
                return;
            }
            for (const agent of agents) {
                if (!agent.misePackages.every((p) => packages.includes(miseKey(p)))) continue;
                try {
                    await provisionAgent(agent, sandbox.workspace_path, runningModelId);
                } catch {
                    // Best-effort; one unwritable workspace file shouldn't block the other agents.
                }
            }
        }),
    );
}
