import { invoke } from "@tauri-apps/api/core";
import { parseError, type AppErrorKind } from "$lib/errors";
import type { ActivityHeadline } from "$lib/activity/types";
import { prepareMachine } from "$lib/machine/readiness";
import { ensureMachineReadiness, machineReadiness } from "$lib/machine/readiness.svelte";
import type { MachineSetupEvent, ProviderId } from "$lib/machine/types";
import {
    checkPodman,
    podmanNeedsSetup,
    podmanRebootRequired,
    podmanUnavailable,
    setupError,
    setupPodman,
    startError,
    startPodman,
} from "$lib/podman/setup.svelte";

/**
 * First-run setup (machine, then base image) as one shared run.
 *
 * Module-scoped so the run survives the welcome flow's step changes: it is started in the
 * background while the user reads the story, and the two setup cards render this same state
 * instead of starting a second run of their own.
 */
interface MachineRun {
    busy: boolean;
    events: MachineSetupEvent[];
    error: string;
    rebootMessage: string;
}

interface ImageRun {
    probing: boolean;
    ready: boolean;
    busy: boolean;
    error: string;
    probeError: string;
    probeErrorKind: AppErrorKind | null;
}

let machine = $state<MachineRun>({ busy: false, events: [], error: "", rebootMessage: "" });
let image = $state<ImageRun>({
    probing: true,
    ready: false,
    busy: false,
    error: "",
    probeError: "",
    probeErrorKind: null,
});

let background: Promise<void> | null = null;
// Reactive twin of `background`, for the headline's "checking" state before either step starts.
let backgroundActive = $state(false);

export function machineRun(): MachineRun { return machine; }
export function imageRun(): ImageRun { return image; }

/** Brings `provider`'s machine online. Joins nothing: a call while one is running is ignored. */
export async function runMachineSetup(provider: ProviderId): Promise<void> {
    if (machine.busy) return;
    machine = { busy: true, events: [], error: "", rebootMessage: "" };
    const result = await prepareMachine(provider, (e) => {
        machine.events = [...machine.events, e];
        if (e.kind === "rebootRequired") machine.rebootMessage = e.message;
    });
    if (result.ok) {
        await ensureMachineReadiness(provider, true);
    } else if (!result.rebootRequired) {
        machine.error = result.message;
    }
    machine.busy = false;
}

/** Whether the base image already exists, without building it. */
export async function probeImage(): Promise<void> {
    try {
        image.ready = await invoke<boolean>("base_image_ready");
        image.probeError = "";
        image.probeErrorKind = null;
    } catch (e) {
        const err = parseError(e);
        image.probeError = err.message;
        image.probeErrorKind = err.kind;
    } finally {
        image.probing = false;
    }
}

/**
 * Builds the base image. Provisioning the machine is chained in rather than required first: the
 * image cannot build without it (docs/features/welcome-onboarding/02_dev.md, Resolved Decisions).
 */
export async function runImageDownload(): Promise<void> {
    if (image.busy) return;
    image.busy = true;
    image.error = "";
    try {
        await checkPodman();
        if (podmanUnavailable()) {
            const started = podmanNeedsSetup() ? await setupPodman() : await startPodman();
            // The reboot case is rendered from the podman store, which keeps the structured kind.
            if (!started) throw { kind: "Backend", message: setupError() || startError() };
        }
        await invoke("build_base_image");
        await probeImage();
    } catch (e) {
        image.error = parseError(e).message;
    } finally {
        image.busy = false;
    }
}

/**
 * Runs whatever setup is still missing, machine first, without waiting for a click. Stops at the
 * first step that needs the user (a failure or a restart), leaving it to the setup cards.
 * A call while a run is in flight joins that run.
 */
export function startBackgroundSetup(provider: ProviderId = "podman"): Promise<void> {
    backgroundActive = true;
    background ??= (async () => {
        await ensureMachineReadiness(provider);
        if (machineReadiness(provider).error) return;
        if (!machineReadiness(provider).readiness.ready) await runMachineSetup(provider);
        if (!machineReadiness(provider).readiness.ready) return;
        await probeImage();
        if (!image.ready && !image.probeError) await runImageDownload();
    })().finally(() => {
        background = null;
        backgroundActive = false;
    });
    return background;
}

/** Whether both steps are complete, so the user can leave onboarding. */
export function setupComplete(provider: ProviderId = "podman"): boolean {
    const status = machineReadiness(provider);
    return !status.probing && status.readiness.ready && !image.probing && image.ready;
}

/**
 * The setup run in one line, for the activity toggle. `null` while there is nothing to report.
 * TODO(i18n): every string below.
 */
export function setupHeadline(provider: ProviderId = "podman"): ActivityHeadline | null {
    const status = machineReadiness(provider);
    const machineDone = !status.probing && status.readiness.ready;

    if (setupComplete(provider)) {
        return { state: "done", title: "Setup finished", detail: "Everything is ready for your first sandbox." };
    }
    if (machine.rebootMessage || podmanRebootRequired()) {
        return {
            state: "attention",
            title: "Restart needed",
            detail: "Windows needs a restart to finish setting up. You can keep reading first.",
        };
    }
    if (machine.busy) {
        return { state: "running", title: "Setting up in the background", detail: "Step 1 of 2 · Creating your workspace" };
    }
    if (image.busy) {
        return { state: "running", title: "Setting up in the background", detail: "Step 2 of 2 · Downloading what sandboxes need" };
    }
    if (machine.error || status.error || image.error || (machineDone && image.probeError)) {
        return {
            state: "attention",
            title: "Setup needs a hand",
            detail: "Something did not go through. You can retry at the end of the tour.",
        };
    }
    if (backgroundActive) return { state: "running", title: "Setting up in the background", detail: "Checking your computer…" };
    return null;
}
