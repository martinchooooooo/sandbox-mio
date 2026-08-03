import { invoke } from "@tauri-apps/api/core";
import { parseError } from "$lib/errors";
import { createActionRunner } from "$lib/actions.svelte";
import { m } from "$lib/paraglide/messages.js";

// Startup gate: set when `check_podman_ready` reports the machine/socket is down.
let podmanUnavailableState = $state(false);

// First-run gate: set when the default Podman machine has never been created (`machine_needs_init`
// returns true). Takes priority over the plain "Start Podman" screen, since starting a machine that
// was never initialized cannot succeed.
let podmanNeedsSetupState = $state(false);

// Set when `provision_podman` reports a fresh WSL2 install needs a Windows restart before it can
// continue. Shown instead of a generic setup failure so the user knows to reboot, not retry blindly.
let podmanRebootRequiredState = $state(false);

// Not `createActionRunner`-driven: that helper only keeps the error message, but this flow
// needs the structured `kind` to tell a reboot requirement apart from a generic failure.
let setupBusyState = $state(false);
let setupErrorState = $state("");

const startAction = createActionRunner();

export function podmanUnavailable(): boolean { return podmanUnavailableState; }
export function podmanNeedsSetup(): boolean { return podmanNeedsSetupState; }
export function podmanRebootRequired(): boolean { return podmanRebootRequiredState; }
export function setupBusy(): boolean { return setupBusyState; }
export function setupError(): string { return setupErrorState; }
export function startBusy(): boolean { return startAction.busy !== null; }
export function startError(): string { return startAction.error; }

/** Probes Podman, distinguishing "never initialized" from "just stopped". Deliberately re-run by
 * every mount rather than cached: Podman can be stopped or started outside the app between mounts. */
export async function checkPodman(): Promise<void> {
    try {
        await invoke("check_podman_ready");
        podmanUnavailableState = false;
        podmanNeedsSetupState = false;
    } catch (e) {
        if (parseError(e).kind !== "PodmanUnavailable") throw e;
        try {
            podmanNeedsSetupState = await invoke<boolean>("machine_needs_init");
        } catch {
            podmanNeedsSetupState = false;
        }
        podmanUnavailableState = true;
    }
}

/** Starts a stopped machine. Returns whether it succeeded, leaving the follow-up work each caller
 * needs (list reload, image build) to the caller. */
export async function startPodman(): Promise<boolean> {
    const ok = await startAction.run(
        "start",
        () => invoke("start_podman_machine"),
        { message: m.podman_starting() }
    );
    if (ok) podmanUnavailableState = false;
    return ok;
}

/** First-run provisioning (WSL2 + `podman machine init` + start). Returns whether it succeeded;
 * a reboot requirement surfaces through `podmanRebootRequired()` rather than the error string. */
export async function setupPodman(): Promise<boolean> {
    podmanRebootRequiredState = false;
    setupErrorState = "";
    setupBusyState = true;
    try {
        await invoke("provision_podman");
        podmanNeedsSetupState = false;
        podmanUnavailableState = false;
        return true;
    } catch (e) {
        const err = parseError(e);
        podmanRebootRequiredState = err.kind === "PodmanRebootRequired";
        setupErrorState = err.message;
        return false;
    } finally {
        setupBusyState = false;
    }
}
