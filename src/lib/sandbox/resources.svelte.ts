import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { ProviderId } from "$lib/providers";
import { classify, type Health } from "$lib/sandbox/health";

// Module-scoped store; owns the single `resources://snapshot` subscription
// (docs/features/sandbox-resource-monitor/02_dev.md, Resolved Decision 9).

/** Mirrors Rust `services::resources::ResourceSample`. */
export interface ResourceSample {
    atMs: number;
    /** Fraction (0 to 1) of the allotted CPUs; null for the first sample after start. */
    cpu: number | null;
    memoryUsed: number;
    memoryLimit: number;
}

/** Mirrors Rust `services::resources::SandboxResources`. */
export interface SandboxResources {
    name: string;
    provider: ProviderId;
    allottedCpus: number | null;
    memoryIncludesCache: boolean;
    /** Oldest first. */
    samples: ResourceSample[];
}

/** Mirrors Rust `provider::types::EnginePool`. */
export interface EnginePool {
    cpus: number;
    memoryTotal: number;
    /** Counts page cache as used, so it understates what the machine can still give out. */
    memoryFree: number;
}

/** Mirrors Rust `services::resources::ResourceSnapshot`. */
export interface ResourceSnapshot {
    atMs: number;
    sandboxes: SandboxResources[];
    podmanPool: EnginePool | null;
}

let snapshot = $state<ResourceSnapshot | null>(null);
let started = false;

function apply(next: ResourceSnapshot) {
    if (!snapshot || next.atMs >= snapshot.atMs) snapshot = next;
}

/** Subscribes once and hydrates from the backend's latest snapshot. */
export async function ensureResources(): Promise<void> {
    if (started) return;
    started = true;
    try {
        await listen<ResourceSnapshot>("resources://snapshot", (e) => apply(e.payload));
        apply(await invoke<ResourceSnapshot>("get_resource_snapshot"));
    } catch (e) {
        console.error(e);
    }
}

export function resourcesFor(name: string): SandboxResources | undefined {
    return snapshot?.sandboxes.find((s) => s.name === name);
}

export function healthFor(name: string): Health {
    return classify(resourcesFor(name));
}

/** The newest sample of `name`, or undefined while it has none. */
export function latestSampleFor(name: string): ResourceSample | undefined {
    return resourcesFor(name)?.samples.at(-1);
}
