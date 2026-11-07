<script lang="ts">
    import type { MachineSetupEvent, SetupStep } from "./types";
    import { m } from "$lib/paraglide/messages.js";

    // `steps` is the provider's ordered checklist (from `machine_setup_steps`). `events` is the
    // running stream from `prepare_machine`, which ticks each row pending → running → done/skipped/
    // failed — same shape as `$lib/models/RuntimeSteps.svelte`, just against a different id space
    // (Podman's four machine-lifecycle steps, or microsandbox's four).
    let {
        steps,
        events = [],
    }: {
        steps: SetupStep[];
        events?: MachineSetupEvent[];
    } = $props();

    // Step ids are Rust-owned; an id with no entry renders blank.
    const titles: Record<string, () => string> = {
        "wsl-install": m.machine_step_wsl_install_title,
        "machine-init": m.machine_step_machine_init_title,
        "machine-start": m.machine_step_machine_start_title,
        "socket-activate": m.machine_step_socket_activate_title,
        "msb-binary": m.machine_step_msb_binary_title,
        "msb-version": m.machine_step_msb_version_title,
        "hypervisor-platform": m.machine_step_hypervisor_platform_title,
        "msb-doctor": m.machine_step_msb_doctor_title,
    };
    const explains: Record<string, () => string> = {
        "wsl-install": m.machine_step_wsl_install_explain,
        "machine-init": m.machine_step_machine_init_explain,
        "machine-start": m.machine_step_machine_start_explain,
        "socket-activate": m.machine_step_socket_activate_explain,
        "msb-binary": m.machine_step_msb_binary_explain,
        "msb-version": m.machine_step_msb_version_explain,
        "hypervisor-platform": m.machine_step_hypervisor_platform_explain,
        "msb-doctor": m.machine_step_msb_doctor_explain,
    };

    type Status = "pending" | "running" | "done" | "skipped" | "failed";
    interface RowState {
        status: Status;
        detail: string | null;
    }

    // Fold the event stream into a per-id state map. Order in `events` is the source of truth.
    const rowStates = $derived.by(() => {
        const map: Record<string, RowState> = {};
        for (const e of events) {
            switch (e.kind) {
                case "stepStart":
                    map[e.id] = { status: "running", detail: null };
                    break;
                case "stepDone":
                    map[e.id] = { status: "done", detail: e.detail };
                    break;
                case "stepSkipped":
                    map[e.id] = { status: "skipped", detail: null };
                    break;
                case "stepFailed":
                    map[e.id] = { status: "failed", detail: e.error };
                    break;
                case "rebootRequired":
                case "ready":
                    break;
            }
        }
        return map;
    });

    const live = $derived(events.length > 0);

    function icon(status: Status | undefined): string {
        switch (status) {
            case "running": return "◐";
            case "done": return "✓";
            case "skipped": return "↷";
            case "failed": return "✗";
            default: return "○";
        }
    }
    function iconClass(status: Status | undefined): string {
        switch (status) {
            case "done":
            case "skipped": return "text-emerald-600 dark:text-emerald-400";
            case "failed": return "text-destructive";
            case "running": return "text-blue-600 dark:text-blue-400";
            default: return "text-muted-foreground";
        }
    }
</script>

<ol class="flex flex-col gap-2">
    {#each steps as step (step.id)}
        {@const state = rowStates[step.id]}
        <li class="flex gap-2 text-sm" data-step-id={step.id}>
            {#if live}
                <span class={iconClass(state?.status) + " shrink-0 w-4 text-center"}>{icon(state?.status)}</span>
            {/if}
            <div class="flex flex-col">
                <span class="font-medium" data-step-title>{titles[step.id]?.() ?? ""}</span>
                <span class="text-muted-foreground text-xs">{explains[step.id]?.() ?? ""}</span>
                {#if state?.detail}
                    <span class="text-xs text-foreground/80 mt-0.5">{state.detail}</span>
                {/if}
            </div>
        </li>
    {/each}
</ol>
