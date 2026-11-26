import { invoke } from "@tauri-apps/api/core";
import type { Task } from "$lib/activity/types";
import type { EngineKind } from "$lib/models/types";

/**
 * Mirrors Rust `settings::types::AppSettings`. `set_settings` replaces the whole file, so a caller
 * writes back the object it read with only its own fields changed.
 */
export interface AppSettings {
    inferenceEngine: EngineKind;
    llamaGpuLayers: number | null;
    llamaCtxSize: number | null;
    onboardingCompleted: boolean;
    firstOpenedAtMs: number | null;
    /** Per-sandbox override of the native chat's learning mode. */
    chatLearningMode: Record<string, boolean>;
}

export function listTasks(): Promise<Task[]> {
    return invoke<Task[]>("list_tasks");
}

export function getSettings(): Promise<AppSettings> {
    return invoke<AppSettings>("get_settings");
}

export function setSettings(settings: AppSettings): Promise<void> {
    return invoke("set_settings", { settings });
}

export function logMilestone(name: string, elapsedMs: number): Promise<void> {
    return invoke("log_milestone", { name, elapsedMs });
}
