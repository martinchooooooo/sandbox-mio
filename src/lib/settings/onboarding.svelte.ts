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

/**
 * Debug affordance: returns the app to a first launch, including the first-run milestone clock.
 * It deliberately keeps the Podman machine, base image and downloaded models
 * (docs/features/welcome-onboarding/01_ideas.md).
 */
export async function restartOnboarding(): Promise<void> {
    const settings = await invoke<AppSettings>("get_settings");
    await invoke("set_settings", {
        settings: { ...settings, onboardingCompleted: false, firstOpenedAtMs: null },
    });
    // A reload drops the in-memory setup jobs and handoff, as a language change does.
    window.location.assign("/");
}
