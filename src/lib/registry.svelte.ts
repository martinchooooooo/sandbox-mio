import { invoke } from "@tauri-apps/api/core";

export interface RegistryEntry { short: string; description?: string; }

let entries = $state<RegistryEntry[]>([]);
let fetched = false;

export function registryEntries(): RegistryEntry[] { return entries; }

export async function ensureRegistry(sandboxName: string): Promise<void> {
    if (fetched) return;
    fetched = true;
    try {
        entries = await invoke<RegistryEntry[]>("list_registry", { name: sandboxName });
    } catch {
        fetched = false;
    }
}
