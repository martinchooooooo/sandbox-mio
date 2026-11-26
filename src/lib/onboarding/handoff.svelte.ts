import { openAgentChat } from "$lib/api/panes";
import { toast } from "svelte-sonner";
import { parseError } from "$lib/errors";
import { completeOnboarding } from "$lib/settings/onboarding.svelte";
import { refreshSandboxState, sandboxesList } from "$lib/sandbox/state.svelte";
import { openSandbox } from "$lib/workspace/navigation.svelte";
import { FIRST_SANDBOX, modelRun, retryFirstSandbox, sandboxRun, startFirstSandbox } from "./setup.svelte";

export { FIRST_SANDBOX };

type Phase = "idle" | "creating" | "opening" | "failed" | "cancelled";

// Module-scoped so a step change or the back button neither restarts nor duplicates the handoff.
let phase = $state<Phase>("idle");
let error = $state("");

export function handoffPhase(): Phase { return phase; }
export function handoffError(): string { return error; }

// A function, so a check after an await is not narrowed away by the assignments before it.
function isCancelled(): boolean { return phase === "cancelled"; }

/**
 * Opens the first sandbox, finishes onboarding and opens the pi chat. The sandbox itself is built
 * by the background setup (`startFirstSandbox` in `setup.svelte.ts`); this waits for that job, so
 * call it once the setup is ready. A repeat call while running, after success or after a cancel
 * does nothing. A failed create or open leaves onboarding open for `retryHandoff`.
 */
export async function startHandoff(): Promise<void> {
    if (phase === "creating" || phase === "opening" || phase === "cancelled") return;
    phase = "creating";
    error = "";
    try {
        // Joins the background job; starts it only when nothing did (it never recreates `hello-world`).
        await startFirstSandbox();
        if (isCancelled()) return;
        const sandbox = sandboxRun();
        if (sandbox.error) throw { kind: "Backend", message: sandbox.error };
        await refreshSandboxState();
        if (isCancelled()) return;
        const workspacePath = sandboxesList().find((p) => p.name === FIRST_SANDBOX)?.workspace_path;
        // The chat needs pi and a serving model; pi installed without a model would fail its first message.
        const piReady = sandbox.installed && modelRun().ready;
        phase = "opening";
        // Opened before onboarding completes: a failure here leaves the retry UI mounted.
        if (workspacePath) await openSandbox(FIRST_SANDBOX, workspacePath, { restore: false });
        await completeOnboarding();
        if (sandbox.installed && !piReady) {
            toast.info("pi is installed in hello-world but needs a Model before it can chat."); // TODO(i18n)
        }
        if (workspacePath && piReady) {
            // The sandbox is already open, so a chat that does not open is a toast, not a handoff failure.
            await openAgentChat(FIRST_SANDBOX, workspacePath).catch((e) => {
                toast.error(`${FIRST_SANDBOX}: ${parseError(e).message}`); // TODO(i18n)
            });
        }
    } catch (e) {
        if (isCancelled()) return;
        error = parseError(e).message;
        phase = "failed";
    }
}

/**
 * Leaves the create running in the background but skips the automatic open of the sandbox. Final
 * until `retryHandoff`; too late once the sandbox is already opening.
 */
export function cancelHandoff(): void {
    if (phase === "idle" || phase === "creating") phase = "cancelled";
}

/** Tries the handoff again; a failed create is retried first, through the setup job. */
export async function retryHandoff(): Promise<void> {
    if (sandboxRun().error) await retryFirstSandbox();
    phase = "idle";
    return startHandoff();
}
