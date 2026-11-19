import { invoke } from "@tauri-apps/api/core";
import { toast } from "svelte-sonner";
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

// Generous: the create includes the agent install, which can take minutes on a slow connection.
const CREATE_TIMEOUT_MS = 20 * 60 * 1000;

/** Resolves with "" once `hello-world` is listed, or with the reason it will not be. */
function waitForCreate(): Promise<string> {
    return new Promise((resolve) => {
        const deadline = Date.now() + CREATE_TIMEOUT_MS;
        const timer = setInterval(() => {
            const state = creationSettled();
            if (state.error || state.done) {
                clearInterval(timer);
                resolve(state.error);
            } else if (Date.now() > deadline) {
                clearInterval(timer);
                resolve("Creating the sandbox took too long.");
            }
        }, 400);
    });
}

/**
 * Builds the first sandbox, finishes onboarding and opens the pi chat. Call once the machine, image
 * and model are ready; a repeat call while running or after success does nothing. An existing
 * `hello-world` is opened as it is, never recreated (docs/features/fastest-time-to-wow/02_dev.md,
 * OQ-hello-world-special). A failed create or open leaves onboarding open for `retryHandoff`.
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
        // An existing sandbox is opened as it is; its chat is tried whenever the model is up.
        let piReady = modelRun().ready;
        if (!workspacePath) {
            piReady = false;
            // Pre-install is explicit: first run never plans from host state, which could add Claude Code.
            const started = createSandbox({
                name: FIRST_SANDBOX,
                proxy: false,
                agents: modelRun().ready ? ["pi"] : [],
                // The handoff owns the open, so a cancel can really prevent it.
                autoOpen: false,
                onCreated: () => recordMilestone("sandbox-created"),
                onInstalled: (installed) => {
                    piReady = installed.includes("pi");
                    if (piReady) recordMilestone("pi-installed");
                },
            });
            // A refused start is fine only when a create or the sandbox itself is already there to wait on.
            if (!started && !creationSettled().done && !pendingCreates().some((p) => p.name === FIRST_SANDBOX)) {
                throw { kind: "Backend", message: "The sandbox could not be started." };
            }
            const failure = await waitForCreate();
            if (failure) throw { kind: "Backend", message: failure };
            workspacePath = sandboxesList().find((p) => p.name === FIRST_SANDBOX)?.workspace_path;
        }
        if (cancelled) {
            phase = "idle";
            return;
        }
        phase = "opening";
        // Opened before onboarding completes: a failure here leaves the retry UI mounted.
        if (workspacePath) await openSandbox(FIRST_SANDBOX, workspacePath, { restore: false });
        await completeOnboarding();
        if (workspacePath && piReady) {
            // The sandbox is already open, so a chat that does not open is a toast, not a handoff failure.
            await invoke("open_agent_chat", { name: FIRST_SANDBOX, workspacePath }).catch((e) => {
                toast.error(`${FIRST_SANDBOX}: ${parseError(e).message}`); // TODO(i18n)
            });
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
