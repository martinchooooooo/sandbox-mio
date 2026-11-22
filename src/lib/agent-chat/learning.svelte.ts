import { invoke } from "@tauri-apps/api/core";
import { parseError } from "$lib/errors";
import { FIRST_SANDBOX } from "$lib/onboarding/setup.svelte";

/** Per-sandbox learning-mode overrides, persisted in `settings.json` as `chatLearningMode`
 * (`docs/features/chat-agent-anatomy/02_dev.md`, Resolved Decisions 1 and 2). */

interface AppSettings {
    chatLearningMode?: Record<string, boolean>;
    [key: string]: unknown;
}

let overrides = $state<Record<string, boolean>>({});
let loading: Promise<void> | null = null;

/** Loads the override map once; a failed read is retried by the next call. */
export function ensureLearning(): Promise<void> {
    loading ??= invoke<AppSettings>("get_settings")
        .then((settings) => {
            overrides = { ...settings.chatLearningMode, ...overrides };
        })
        .catch((e) => {
            loading = null;
            console.error(parseError(e).message);
        });
    return loading;
}

/** The override for `sandbox`, else on for the onboarding sandbox only. */
export function learningMode(sandbox: string): boolean {
    return overrides[sandbox] ?? sandbox === FIRST_SANDBOX;
}

/** Round-trips the whole settings object: `set_settings` replaces the file, so a partial write
 * would drop every other field. */
export async function setLearningMode(sandbox: string, on: boolean): Promise<void> {
    const previous = overrides[sandbox];
    overrides = { ...overrides, [sandbox]: on };
    try {
        const settings = await invoke<AppSettings>("get_settings");
        const chatLearningMode = { ...settings.chatLearningMode, [sandbox]: on };
        await invoke("set_settings", { settings: { ...settings, chatLearningMode } });
    } catch (e) {
        const { [sandbox]: _, ...rest } = overrides;
        overrides = previous === undefined ? rest : { ...rest, [sandbox]: previous };
        throw e;
    }
}
