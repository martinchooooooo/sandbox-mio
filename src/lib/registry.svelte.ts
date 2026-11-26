import { listRegistry } from "$lib/api/packages";

export interface RegistryEntry { short: string; description?: string; }

let entries = $state<RegistryEntry[]>([]);
let fetched = false;

export function registryEntries(): RegistryEntry[] { return entries; }

export async function ensureRegistry(sandboxName: string): Promise<void> {
    if (fetched) return;
    fetched = true;
    try {
        entries = await listRegistry(sandboxName);
    } catch {
        fetched = false;
    }
}
