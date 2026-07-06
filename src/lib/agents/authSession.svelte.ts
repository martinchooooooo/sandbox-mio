import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AgentAuthEvent } from "./types";

// Module-scoped store (mirrors src/lib/workspace/state.svelte.ts): owns the single
// `agent-auth-output-<sessionId>` subscription for the in-progress login, so
// credentials/+page.svelte reads accessors instead of calling `listen()` itself.

export type AuthPhase = "idle" | "starting" | "awaiting-url" | "awaiting-code" | "exited";

let sessionId = $state<string | null>(null);
let phase = $state<AuthPhase>("idle");
let outputLines = $state<string[]>([]);
let authUrl = $state<string | null>(null);
let exitCode = $state<number | null>(null);

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

export function authExitCode(): number | null {
    return exitCode;
}

/** Marks a login as in flight before a sessionId exists (during ensure_auth_container/install_agent). */
export function beginSigningIn(): void {
    phase = "starting";
    outputLines = [];
    authUrl = null;
    exitCode = null;
}

/** Attaches the session's output listener once start_agent_auth has returned a sessionId. */
export async function start(id: string): Promise<void> {
    sessionId = id;
    phase = "awaiting-url";
    unlisten = await listen<AgentAuthEvent>(`agent-auth-output-${id}`, (event) => {
        const data = event.payload;
        if (data.kind === "Output") {
            outputLines = [...outputLines, data.line];
            const match = data.line.match(/https:\/\/[a-z0-9.-]+\.[a-z]{2,}\/[^\s]+/i);
            if (match && !authUrl) authUrl = match[0];
            if (data.line.toLowerCase().includes("paste code")) phase = "awaiting-code";
        } else if (data.kind === "Exit") {
            exitCode = data.code;
            phase = "exited";
        }
    });
}

/** Tears down the listener and clears all state. Called once the credentials page has finished
 * its own Exit handling (capture/seed/destroy) and the card is dismissed. */
export function reset(): void {
    unlisten?.();
    unlisten = null;
    sessionId = null;
    phase = "idle";
    outputLines = [];
    authUrl = null;
    exitCode = null;
}
