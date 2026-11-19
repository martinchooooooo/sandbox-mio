import { invoke } from "@tauri-apps/api/core";
import { parseError } from "$lib/errors";
import { completeOnboarding } from "$lib/settings/onboarding.svelte";
import { createSandbox, dismissCreate, pendingCreates } from "$lib/sandbox/lifecycle.svelte";
import { refreshSandboxState, sandboxesList } from "$lib/sandbox/state.svelte";
import { openSandbox } from "$lib/workspace/navigation.svelte";
import { modelRun } from "./setup.svelte";
import { recordMilestone } from "./milestones";

/** The first sandbox's name; the dash is required by `validate_name`, so "hello world" is copy only. */
export const FIRST_SANDBOX = "hello-world";

type Phase = "idle" | "creating" | "opening" | "failed";

// Module-scoped so a step change or the back button neither restarts nor duplicates the handoff.
let phase = $state<Phase>("idle");
let error = $state("");
let step = $state("");
let cancelled = false;

export function handoffPhase(): Phase { return phase; }
export function handoffError(): string { return error; }
export function handoffStep(): string { return step; }

function creationSettled(): { done: boolean; error: string } {
    const placeholder = pendingCreates().find((p) => p.name === FIRST_SANDBOX);
    if (placeholder) {
        step = placeholder.step;
        return { done: false, error: placeholder.error };
    }
    return { done: sandboxesList().some((p) => p.name === FIRST_SANDBOX), error: "" };
}

function waitForCreate(): Promise<string> {
    return new Promise((resolve) => {
        const timer = setInterval(() => {
            const state = creationSettled();
            if (state.error || state.done) {
                clearInterval(timer);
                resolve(state.error);
            }
        }, 400);
    });
}

/**
 * Builds the first sandbox, finishes onboarding and opens the pi chat. Call once the machine, image
 * and model are ready; a repeat call while running or after success does nothing. An existing
 * `hello-world` is opened as it is, never recreated (docs/features/fastest-time-to-wow/02_dev.md,
 * OQ-hello-world-special). A failed create leaves onboarding open for `retryHandoff`.
 */
export async function startHandoff(): Promise<void> {
    if (phase === "creating" || phase === "opening") return;
    phase = "creating";
    cancelled = false;
    error = "";
    step = "";
    try {
        await refreshSandboxState();
        let workspacePath = sandboxesList().find((p) => p.name === FIRST_SANDBOX)?.workspace_path;
        if (!workspacePath) {
            // Pre-install is explicit: first run never plans from host state, which could add Claude Code.
            createSandbox({ name: FIRST_SANDBOX, proxy: false, agents: modelRun().ready ? ["pi"] : [] });
            const failure = await waitForCreate();
            if (failure) throw { kind: "Backend", message: failure };
            recordMilestone("sandbox-created");
            if (modelRun().ready) recordMilestone("pi-installed");
            workspacePath = sandboxesList().find((p) => p.name === FIRST_SANDBOX)?.workspace_path;
        }
        if (cancelled) {
            phase = "idle";
            return;
        }
        phase = "opening";
        await completeOnboarding();
        if (workspacePath) {
            await openSandbox(FIRST_SANDBOX, workspacePath, { restore: false });
            if (modelRun().ready) {
                // The sandbox is already open; a missing chat pane (pi failed to install) is not a handoff failure.
                await invoke("open_agent_chat", { name: FIRST_SANDBOX, workspacePath }).catch(() => {});
            }
        }
    } catch (e) {
        error = parseError(e).message;
        phase = "failed";
    }
}

/** Leaves the create running in the background but skips the automatic open of the sandbox. */
export function cancelHandoff(): void {
    cancelled = true;
}

/** Dismisses a failed create's placeholder and tries again. */
export function retryHandoff(): Promise<void> {
    dismissCreate(FIRST_SANDBOX);
    phase = "idle";
    return startHandoff();
}
