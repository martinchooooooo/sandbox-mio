import { invoke } from "@tauri-apps/api/core";
import { toast } from "svelte-sonner";
import { parseError } from "$lib/errors";
import { completeOnboarding } from "$lib/settings/onboarding.svelte";
import { refreshSandboxState, sandboxesList } from "$lib/sandbox/state.svelte";
import { openSandbox } from "$lib/workspace/navigation.svelte";
import { FIRST_SANDBOX, modelRun, retryFirstSandbox, sandboxRun, startFirstSandbox } from "./setup.svelte";

export { FIRST_SANDBOX };

type Phase = "idle" | "creating" | "opening" | "failed";

// Module-scoped so a step change or the back button neither restarts nor duplicates the handoff.
let phase = $state<Phase>("idle");
let error = $state("");
let cancelled = false;

export function handoffPhase(): Phase { return phase; }
export function handoffError(): string { return error; }

/**
 * Opens the first sandbox, finishes onboarding and opens the pi chat. The sandbox itself is built
 * by the background setup (`startFirstSandbox` in `setup.svelte.ts`); this waits for that job, so
 * call it once the setup is ready. A repeat call while running or after success does nothing. A
 * failed create or open leaves onboarding open for `retryHandoff`.
 */
export async function startHandoff(): Promise<void> {
    if (phase === "creating" || phase === "opening") return;
    phase = "creating";
    cancelled = false;
    error = "";
    try {
        // Joins the background job; starts it only when nothing did (it never recreates `hello-world`).
        await startFirstSandbox();
        const sandbox = sandboxRun();
        if (sandbox.error) throw { kind: "Backend", message: sandbox.error };
        await refreshSandboxState();
        const workspacePath = sandboxesList().find((p) => p.name === FIRST_SANDBOX)?.workspace_path;
        // An existing sandbox is opened as it is; its chat is tried whenever the model is up.
        const piReady = sandbox.reused ? modelRun().ready : sandbox.installed;
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

/** Tries the handoff again; a failed create is retried first, through the setup job. */
export async function retryHandoff(): Promise<void> {
    if (sandboxRun().error) await retryFirstSandbox();
    phase = "idle";
    return startHandoff();
}
