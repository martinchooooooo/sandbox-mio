<script lang="ts">
    import { m } from "$lib/paraglide/messages.js";
    import type { SetupStep, RuntimeSetupEvent } from "./types";

    // `steps` is the engine's ordered checklist (from `runtime_setup_steps`). In live mode,
    // `events` is the running stream of `RuntimeSetupEvent`s from `start_inference`, which ticks
    // each row pending → running → done/skipped/failed. Static mode (settings explainer) passes
    // no events and the rows render as a plain plan.
    let {
        steps,
        events = [],
    }: {
        steps: SetupStep[];
        events?: RuntimeSetupEvent[];
    } = $props();

    // Step ids are engine-owned (Rust); copy lives in Paraglide. Paraglide generates one function
    // per key, so an explicit id→message map is required rather than dynamic indexing of `m`.
    // A new step id with no entry here renders blank — guarded by the e2e checklist assertion.
    const titles: Record<string, () => string> = {
        "detect-device": m.runtime_step_detect_device_title,
        "engine-ready": m.runtime_step_engine_ready_title,
        "model-ready": m.runtime_step_model_ready_title,
        "launch-server": m.runtime_step_launch_server_title,
        "health-check": m.runtime_step_health_check_title,
        "check-foundry": m.runtime_step_check_foundry_title,
        "register-eps": m.runtime_step_register_eps_title,
        "start-service": m.runtime_step_start_service_title,
        "load-model": m.runtime_step_load_model_title,
        "resolve-id": m.runtime_step_resolve_id_title,
    };
    const explains: Record<string, () => string> = {
        "detect-device": m.runtime_step_detect_device_explain,
        "engine-ready": m.runtime_step_engine_ready_explain,
        "model-ready": m.runtime_step_model_ready_explain,
        "launch-server": m.runtime_step_launch_server_explain,
        "health-check": m.runtime_step_health_check_explain,
        "check-foundry": m.runtime_step_check_foundry_explain,
        "register-eps": m.runtime_step_register_eps_explain,
        "start-service": m.runtime_step_start_service_explain,
        "load-model": m.runtime_step_load_model_explain,
        "resolve-id": m.runtime_step_resolve_id_explain,
    };

    type Status = "pending" | "running" | "done" | "skipped" | "failed";
    interface RowState {
        status: Status;
        detail: string | null;
    }

    function title(id: string): string {
        return titles[id]?.() ?? "";
    }
    function explain(id: string): string {
        return explains[id]?.() ?? "";
    }

    // Fold the event stream into a per-id state map. Order in `events` is the source of truth.
    const rowStates = $derived.by(() => {
        const map: Record<string, RowState> = {};
        for (const e of events) {
            switch (e.kind) {
                case "stepStart":
                    map[e.id] = { status: "running", detail: null };
                    break;
                case "stepProgress":
                    // Liveness heartbeat (e.g. elapsed seconds) while a long step runs; keep the
                    // row "running" and surface the latest detail so a slow step looks alive.
                    map[e.id] = { status: "running", detail: e.detail };
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
                <span class="font-medium" data-step-title>{title(step.id)}</span>
                <span class="text-muted-foreground text-xs">{explain(step.id)}</span>
                {#if state?.detail}
                    <span class="text-xs text-foreground/80 mt-0.5">{state.detail}</span>
                {/if}
            </div>
        </li>
    {/each}
</ol>
