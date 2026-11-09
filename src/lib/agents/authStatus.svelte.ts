import { invoke } from "@tauri-apps/api/core";
import type { AgentAuthStatus, AgentDescriptor } from "$lib/agents/types";

// Module-scoped per-sandbox cache so a remounted Overview shows the last known login state at once
// and revalidates behind it.

const statusBySandbox = $state<Record<string, Record<string, AgentAuthStatus>>>({});

const EMPTY: Record<string, AgentAuthStatus> = {};

/** Last known auth status per agent id for `sandboxName`. */
export function agentAuthFor(sandboxName: string): Record<string, AgentAuthStatus> {
    return statusBySandbox[sandboxName] ?? EMPTY;
}

async function checkAgent(sandboxName: string, agent: AgentDescriptor): Promise<void> {
    const auth = agent.auth;
    if (!auth) return;
    let next: AgentAuthStatus;
    try {
        const result = await invoke<AgentAuthStatus>("get_agent_auth_status", {
            name: sandboxName,
            credentialPath: auth.credentialFile,
            expiresAtJsonKey: auth.expiresAtJsonKey,
            accountSourcePath: auth.accountSourceFile ?? null,
            accountEmailJsonKey: auth.accountEmailJsonKey ?? null,
        });
        const expiresAt = result.expiresAtMs;
        next = {
            authenticated: result.authenticated && (expiresAt === null || expiresAt > Date.now()),
            expiresAtMs: expiresAt,
            accountEmail: result.accountEmail,
        };
    } catch {
        next = { authenticated: false, expiresAtMs: null, accountEmail: null };
    }
    statusBySandbox[sandboxName] = { ...statusBySandbox[sandboxName], [agent.id]: next };
}

/** Checks every agent concurrently, writing each agent's entry as its own call settles. */
export async function refreshAgentAuth(sandboxName: string, agents: AgentDescriptor[]): Promise<void> {
    await Promise.all(agents.map((agent) => checkAgent(sandboxName, agent)));
}

export function forgetAgentAuth(sandboxName: string): void {
    delete statusBySandbox[sandboxName];
}
