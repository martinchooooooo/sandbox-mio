// Copy for the Models page. Technical names (llama.cpp, Vulkan, RAM, GGUF) stay out of visible text.
// Everything is a function or getter so the text follows the active locale at render time.
import { m } from "$lib/paraglide/messages.js";
import type { Fit } from "./recommend";
import type { EngineKind, FitGroup, FitVerdict, GoodFor } from "./types";

const SUMMARIES: Record<string, () => string> = {
    "qwen3.5-2b": m.models_summary_qwen3_5_2b,
    "qwen3.5-4b": m.models_summary_qwen3_5_4b,
    "qwen3.5-9b": m.models_summary_qwen3_5_9b,
    "gemma-4-12b-it": m.models_summary_gemma_4_12b_it,
    "gpt-oss-20b": m.models_summary_gpt_oss_20b,
    "devstral-small-2-24b": m.models_summary_devstral_small_2_24b,
    "qwen3.8-27b": m.models_summary_qwen3_8_27b,
    "qwen3.6-35b-a3b": m.models_summary_qwen3_6_35b_a3b,
    "test-model": m.models_summary_test_model,
};

export function modelSummary(id: string): string {
    return SUMMARIES[id]?.() ?? "";
}

const GOOD_FOR: Record<GoodFor, () => string> = {
    "quick-replies": m.models_good_for_quick_replies,
    "everyday-chat": m.models_good_for_everyday_chat,
    "coding-agent": m.models_good_for_coding_agent,
    "careful-thinking": m.models_good_for_careful_thinking,
    "long-documents": m.models_good_for_long_documents,
};

export function goodForLabel(tag: GoodFor): string {
    return GOOD_FOR[tag]();
}

const SIZES = [m.models_size_small, m.models_size_medium, m.models_size_large, m.models_size_very_large];
const SPEEDS = [m.models_speed_quick, m.models_speed_steady, m.models_speed_slow];

export const SPEED_STEPS = SPEEDS.length;
export const sizeLabel = (step: number): string => SIZES[step]();
export const speedLabel = (step: number): string => SPEEDS[step]();

const FIT_LABELS: Record<FitVerdict, () => string> = {
    fits: m.models_fit_label_fits,
    tight: m.models_fit_label_tight,
    busy: m.models_fit_label_busy,
    "too-small": m.models_fit_label_too_small,
    engine: m.models_fit_label_engine,
};

export const fitLabel = (verdict: FitVerdict): string => FIT_LABELS[verdict]();

export function fitSentence(f: Fit): string {
    switch (f.verdict) {
        case "fits": return m.models_fit_sentence_fits();
        case "tight": return m.models_fit_sentence_tight();
        case "busy": return m.models_fit_sentence_busy({ need: f.needGb, free: f.freeGb });
        case "too-small": return m.models_fit_sentence_too_small({ need: f.needGb, total: f.totalGb });
        case "engine": return m.models_fit_sentence_engine();
    }
}

const GROUPS: Record<FitGroup, () => string> = {
    "runs-well": m.models_group_runs_well,
    "runs-slowly": m.models_group_runs_slowly,
    "needs-more": m.models_group_needs_more,
};

export const groupTitle = (group: FitGroup): string => GROUPS[group]();

// Plain titles of the engine-owned setup steps, mirroring `titles` in RuntimeSteps.svelte.
const STEP_TITLES: Record<string, () => string> = {
    "detect-device": m.models_step_detect_device,
    "engine-ready": m.models_step_engine_ready,
    "model-ready": m.models_step_model_ready,
    "launch-server": m.models_step_launch_server,
    "health-check": m.models_step_health_check,
    "check-foundry": m.models_step_check_foundry,
    "register-eps": m.models_step_register_eps,
    "start-service": m.models_step_start_service,
    "load-model": m.models_step_load_model,
    "resolve-id": m.models_step_resolve_id,
};

export const planStepTitle = (stepId: string): string | undefined => STEP_TITLES[stepId]?.();

const EXPLAINERS = {
    model: [m.models_explainer_model_title, m.models_explainer_model_body],
    memory: [m.models_explainer_memory_title, m.models_explainer_memory_body],
    download: [m.models_explainer_download_title, m.models_explainer_download_body],
    speed: [m.models_explainer_speed_title, m.models_explainer_speed_body],
    size: [m.models_explainer_size_title, m.models_explainer_size_body],
    pick: [m.models_explainer_pick_title, m.models_explainer_pick_body],
    cantRun: [m.models_explainer_cant_run_title, m.models_explainer_cant_run_body],
} as const;

export type ExplainerKey = keyof typeof EXPLAINERS;

export const explainer = (key: ExplainerKey): { title: string; body: string } => ({
    title: EXPLAINERS[key][0](),
    body: EXPLAINERS[key][1](),
});

// Product names, not translated.
export const ENGINE_NAMES: Record<EngineKind, string> = { llama: "llama.cpp with Vulkan", foundry: "Microsoft Foundry Local" };

export const PAGE = {
    get title() { return m.models_heading(); },
    get intro() { return m.models_intro(); },
    get yourModel() { return m.models_your_model(); },
    get yourComputer() { return m.models_your_computer(); },
    get otherModels() { return m.models_other_models(); },
    get pickBadge() { return m.models_pick_badge(); },
    get notDownloaded() { return m.models_not_downloaded(); },
    downloading: (pct: number) => m.models_downloading({ percent: pct }),
    get asleep() { return m.models_asleep(); },
    get awake() { return m.models_awake(); },
    get waking() { return m.models_waking(); },
    get ready() { return m.models_ready(); },
    get failed() { return m.models_failed(); },
    get downloaded() { return m.models_downloaded(); },
    get downloadAndWake() { return m.models_download_and_wake(); },
    get wake() { return m.models_wake(); },
    get sleep() { return m.models_sleep(); },
    get tryAnyway() { return m.models_try_anyway(); },
    get useThis() { return m.models_use_this(); },
    get downloadAnyway() { return m.models_download_anyway(); },
    get sandboxesUse() { return m.models_sandboxes_use(); },
    get openSandbox() { return m.models_open_sandbox(); },
    computer: (totalGb: number, freeGb: number, gpu: boolean) =>
        gpu ? m.models_computer_gpu({ total: totalGb, free: freeGb }) : m.models_computer_no_gpu({ total: totalGb, free: freeGb }),
    get wakeSteps() { return m.models_wake_steps(); },
    get size() { return m.models_size_label(); },
    get speed() { return m.models_speed_label(); },
    download: (gb: number) => m.models_download_size({ gb }),
    memory: (gb: number) => m.models_memory_need({ gb }),
};
