import { Channel, invoke } from "@tauri-apps/api/core";
import type { AgentAuthStatus, AgentInstallEvent, OnboardingSeed } from "$lib/agents/types";

export interface InstallAgentArgs {
    name: string;
    packages: string[];
    /** `[npm package, node version]` for a post-install npm step. */
    postinstallNpm: [string, string] | null;
    credentialService?: string | null;
    onboarding?: OnboardingSeed | null;
    onProgress: Channel<AgentInstallEvent>;
}

export function installAgent(args: InstallAgentArgs): Promise<void> {
    return invoke("install_agent", { ...args });
}

export interface ProvisionOpencodeArgs {
    workspacePath: string;
    relativePath: string;
    providerId: string;
    providerJson: unknown;
    defaultModel: string | null;
}

export function provisionOpencode(args: ProvisionOpencodeArgs): Promise<void> {
    return invoke("provision_opencode", { ...args });
}

export interface ProvisionPiArgs extends ProvisionOpencodeArgs {
    defaultProvider: string | null;
    configEnv: string;
    configDirContainer: string;
}

export function provisionPi(args: ProvisionPiArgs): Promise<void> {
    return invoke("provision_pi", { ...args });
}

/** Returns the auth session id that `sendAgentAuthInput` and `cancelAgentAuth` address. */
export function startAgentAuth(name: string, authCommand: string[]): Promise<string> {
    return invoke<string>("start_agent_auth", { name, authCommand });
}

export function sendAgentAuthInput(sessionId: string, input: string): Promise<void> {
    return invoke("send_agent_auth_input", { sessionId, input });
}

export function cancelAgentAuth(sessionId: string): Promise<void> {
    return invoke("cancel_agent_auth", { sessionId });
}

export interface AgentAuthStatusArgs {
    name: string;
    credentialPath: string;
    expiresAtJsonKey: string;
    accountSourcePath: string | null;
    accountEmailJsonKey: string | null;
}

export function getAgentAuthStatus(args: AgentAuthStatusArgs): Promise<AgentAuthStatus> {
    return invoke<AgentAuthStatus>("get_agent_auth_status", { ...args });
}
