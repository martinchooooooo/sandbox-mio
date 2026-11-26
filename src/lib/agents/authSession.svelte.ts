import { destroySandbox } from "$lib/api/sandbox";
import { captureAgentCredentials, captureGithubLogin, seedAgentOnboarding } from "$lib/api/credentials";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { toast } from "svelte-sonner";
import { parseError } from "$lib/errors";
import { GITHUB_AUTH_CONTAINER, GITHUB_SERVICE, parseDeviceCode } from "$lib/github";
import { AGENTS, onboardingSeed } from "./catalogue";
import type { AgentAuthEvent } from "./types";

// Module-scoped store; owns the subscription and the exit cleanup for the in-progress login
// session, so the auth container is destroyed even when the credentials page is unmounted.

export type AuthPhase = "idle" | "starting" | "awaiting-url" | "awaiting-code" | "exited";

let sessionId = $state<string | null>(null);
let phase = $state<AuthPhase>("idle");
let outputLines = $state<string[]>([]);
let authUrl = $state<string | null>(null);
let deviceCode = $state<string | null>(null);
let exitCode = $state<number | null>(null);
let activeServiceId = $state<string | null>(null);
let capturedCount = $state(0);
let handledExitId: string | null = null;

let unlisten: UnlistenFn | null = null;

export function authSessionId(): string | null {
    return sessionId;
}

export function authPhase(): AuthPhase {
    return phase;
}

export function authOutputLines(): string[] {
    return outputLines;
}

export function authUrlDetected(): string | null {
    return authUrl;
}

/** One-time code of a GitHub device-flow login, shown for the user to enter in the browser. */
export function authDeviceCode(): string | null {
    return deviceCode;
}

export function authExitCode(): number | null {
    return exitCode;
}

/** Credential service the in-progress login belongs to; null when no login is active. */
export function authActiveServiceId(): string | null {
    return activeServiceId;
}

/** Increments after each successful capture, so pages can refresh their bundle list. */
export function authCapturedCount(): number {
    return capturedCount;
}

/** Marks a login for `serviceId` as in flight before a sessionId exists (during ensure_auth_container/install_agent). */
export function beginSigningIn(serviceId: string): void {
    activeServiceId = serviceId;
    phase = "starting";
    outputLines = [];
    authUrl = null;
    deviceCode = null;
    exitCode = null;
}

/** Attaches the session's output listener once start_agent_auth has returned a sessionId. */
export async function start(id: string): Promise<void> {
    unlisten?.();
    unlisten = null;
    sessionId = id;
    phase = "awaiting-url";
    unlisten = await listen<AgentAuthEvent>(`agent-auth-output-${id}`, (event) => {
        const data = event.payload;
        if (data.kind === "Output") {
            outputLines = [...outputLines, data.line];
            const match = data.line.match(/https:\/\/[a-z0-9.-]+\.[a-z]{2,}\/[^\s]+/i);
            if (match && !authUrl) authUrl = match[0];
            const code = parseDeviceCode(data.line);
            if (code) deviceCode = code;
            if (data.line.toLowerCase().includes("paste code")) phase = "awaiting-code";
        } else if (data.kind === "Exit") {
            exitCode = data.code;
            phase = "exited";
            if (id !== handledExitId) {
                handledExitId = id;
                void handleExit();
            }
        }
    });
}

async function handleExit(): Promise<void> {
    const serviceId = activeServiceId;
    if (!serviceId) return;
    const isGithub = serviceId === GITHUB_SERVICE;
    const agent = AGENTS.find((a) => a.credentialService === serviceId && a.auth);
    if (!isGithub && (!agent || !agent.auth)) return;
    const authContainer = isGithub ? GITHUB_AUTH_CONTAINER : `mio-auth-${serviceId}`;
    const code = exitCode;

    if (code === 0) {
        try {
            if (isGithub) {
                await captureGithubLogin();
            } else {
                const auth = agent!.auth!;
                const seed = onboardingSeed(agent!);
                if (seed) {
                    await seedAgentOnboarding(authContainer, seed).catch(
                        () => {},
                    );
                }
                await captureAgentCredentials({
                    name: authContainer,
                    serviceId,
                    credentialPath: auth.credentialFile,
                    expiresAtJsonKey: auth.expiresAtJsonKey,
                    accountSourcePath: auth.accountSourceFile ?? null,
                    accountEmailJsonKey: auth.accountEmailJsonKey ?? null,
                });
            }
            capturedCount++;
        } catch (e) {
            // A failed capture must still let the card collapse below — otherwise a CLI login
            // that printed success leaves the row stuck open with nothing rendered, which is
            // indistinguishable from a hang.
            toast.error(parseError(e).message);
        }
    }

    // Destroyed after every attempt, whatever the outcome, so a failed capture never leaks a
    // container (docs/features/shared-agent-auth/02_dev.md, "Auth container cleanup on failure").
    await destroySandbox(authContainer, true).catch(() => {});

    if (code === 0) reset();
}

/** Tears down the listener and clears all state, including the active service. Called when the
 * card is dismissed or after a successful exit has been handled. */
export function reset(): void {
    activeServiceId = null;
    unlisten?.();
    unlisten = null;
    sessionId = null;
    phase = "idle";
    outputLines = [];
    authUrl = null;
    deviceCode = null;
    exitCode = null;
}
