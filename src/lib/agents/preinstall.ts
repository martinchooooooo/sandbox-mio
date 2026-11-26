import { listCredentialBundles } from "$lib/api/credentials";
import { downloadedModels as fetchDownloadedModels } from "$lib/api/inference";
import { AGENTS } from "./catalogue";
import type { AgentDescriptor } from "./types";

/** Host facts the pre-install planner decides from. */
export interface PreinstallFacts {
    /** Ids from `downloaded_models`. */
    downloadedModels: string[];
    /** Service ids from `list_credential_bundles`. */
    credentialServices: string[];
}

/**
 * Agent ids a new sandbox gets automatically: pi once any model is downloaded, Claude Code once
 * the credential store holds an Anthropic login. Pure, so it needs no IPC to test.
 */
export function planPreinstall(facts: PreinstallFacts): string[] {
    const ids: string[] = [];
    if (facts.downloadedModels.length > 0) ids.push("pi");
    if (facts.credentialServices.includes("anthropic")) ids.push("claude-code");
    return ids;
}

/** Reads the host facts; an unreadable source counts as empty so planning never fails a create. */
export async function readPreinstallFacts(): Promise<PreinstallFacts> {
    const [downloadedModels, bundles] = await Promise.all([
        fetchDownloadedModels().catch(() => [] as string[]),
        listCredentialBundles().catch(() => []),
    ]);
    return { downloadedModels, credentialServices: bundles.map(([service]) => service) };
}

export function agentsById(ids: string[]): AgentDescriptor[] {
    return ids.flatMap((id) => AGENTS.filter((a) => a.id === id));
}
