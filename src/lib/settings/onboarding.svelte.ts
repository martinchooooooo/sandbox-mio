import { invoke } from "@tauri-apps/api/core";

interface AppSettings {
    onboardingCompleted: boolean;
    [key: string]: unknown;
}

// `undefined` = not loaded yet, distinct from `false` = loaded and incomplete, so the layout can
// hold the chrome decision until the first `get_settings` resolves.
let completed = $state<boolean | undefined>(undefined);
let fetched = false;

export function onboardingCompleted(): boolean | undefined { return completed; }

export async function ensureOnboardingState(): Promise<void> {
    if (fetched) return;
    fetched = true;
    try {
        completed = (await invoke<AppSettings>("get_settings")).onboardingCompleted;
    } catch {
        fetched = false;
    }
}

/** Round-trips the whole settings object: `set_settings` replaces the file, so a partial write
 * would drop every other field. */
async function writeCompleted(value: boolean): Promise<void> {
    const settings = await invoke<AppSettings>("get_settings");
    await invoke("set_settings", { settings: { ...settings, onboardingCompleted: value } });
    completed = value;
    fetched = true;
}

export function completeOnboarding(): Promise<void> {
    return writeCompleted(true);
}

/** Debug affordance: replays the welcome screens only. It deliberately does not undo the Podman
 * machine or the base image (docs/features/welcome-onboarding/01_ideas.md). */
export function resetOnboarding(): Promise<void> {
    return writeCompleted(false);
}
