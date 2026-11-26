import { invoke } from "@tauri-apps/api/core";
import type { OnboardingSeed } from "$lib/agents/types";

/** Mirrors Rust `credentials::types::CredentialManifest`; the stored secret itself never reaches the frontend. */
export interface CredentialManifest {
    capturedAtMs: number;
    accountEmail: string | null;
    expiresAtMs: number | null;
    accountLogin: string | null;
    missingScopes: string[];
}

/** Mirrors Rust `credentials::github::ApplyOutcome`. */
export type GithubApplyOutcome =
    | { kind: "applied"; capturedAtMs: number }
    | { kind: "needsImage" }
    | { kind: "failed"; reason: string; capturedAtMs: number };

/** Mirrors Rust `services::github_access::GithubSandboxStatus`. */
export interface GithubSandboxStatus {
    optedOut: boolean;
    outcome: GithubApplyOutcome | null;
    credentialCapturedAtMs: number | null;
    accountLogin: string | null;
    deployed: boolean;
}

export function ensureAuthContainer(name: string): Promise<void> {
    return invoke("ensure_auth_container", { name });
}

export interface CaptureAgentCredentialsArgs {
    name: string;
    serviceId: string;
    credentialPath: string;
    expiresAtJsonKey: string;
    accountSourcePath: string | null;
    accountEmailJsonKey: string | null;
}

export function captureAgentCredentials(args: CaptureAgentCredentialsArgs): Promise<CredentialManifest> {
    return invoke<CredentialManifest>("capture_agent_credentials", { ...args });
}

export function injectAgentCredentials(
    name: string,
    serviceId: string,
    onboarding: OnboardingSeed | null | undefined,
): Promise<void> {
    return invoke("inject_agent_credentials", { name, serviceId, onboarding });
}

export function seedAgentOnboarding(name: string, onboarding: OnboardingSeed): Promise<void> {
    return invoke("seed_agent_onboarding", { name, onboarding });
}

export function listCredentialBundles(): Promise<[string, CredentialManifest][]> {
    return invoke<[string, CredentialManifest][]>("list_credential_bundles");
}

export function deleteCredentialBundle(serviceId: string): Promise<void> {
    return invoke("delete_credential_bundle", { serviceId });
}

export function setGithubToken(token: string): Promise<CredentialManifest> {
    return invoke<CredentialManifest>("set_github_token", { token });
}

export function captureGithubLogin(): Promise<CredentialManifest> {
    return invoke<CredentialManifest>("capture_github_login");
}

export function githubSandboxStatus(name: string): Promise<GithubSandboxStatus> {
    return invoke<GithubSandboxStatus>("github_sandbox_status", { name });
}

export function setGithubSandboxAccess(name: string, enabled: boolean): Promise<GithubSandboxStatus> {
    return invoke<GithubSandboxStatus>("set_github_sandbox_access", { name, enabled });
}
