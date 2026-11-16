import type { SandboxResources } from "$lib/sandbox/resources.svelte";

// Pure health classification over a sandbox's recent samples
// (docs/features/sandbox-resource-monitor/02_dev.md, Health).

export type Health = "unknown" | "ok" | "busy" | "strained";

/** Equal to `PRESSURE_FRACTION` in `src-tauri/src/services/resources.rs`. */
export const STRAINED_MEMORY = 0.9;
export const BUSY_MEMORY = 0.75;
export const BUSY_CPU = 0.9;

/** Samples classified over; fewer than `MIN_SAMPLES` leaves the sandbox unknown. */
const WINDOW = 3;
const MIN_SAMPLES = 2;

function memoryFraction(s: { memoryUsed: number; memoryLimit: number }): number {
    return s.memoryLimit > 0 ? s.memoryUsed / s.memoryLimit : 0;
}

function mean(values: number[]): number | null {
    return values.length === 0 ? null : values.reduce((a, b) => a + b, 0) / values.length;
}

export function classify(resources: SandboxResources | undefined): Health {
    if (!resources || resources.samples.length < MIN_SAMPLES) return "unknown";
    const recent = resources.samples.slice(-WINDOW);
    if (recent.length === WINDOW && recent.every((s) => memoryFraction(s) >= STRAINED_MEMORY)) {
        return "strained";
    }
    const memory = mean(recent.map(memoryFraction)) ?? 0;
    const cpu = mean(recent.flatMap((s) => (s.cpu === null ? [] : [s.cpu])));
    if (memory >= BUSY_MEMORY || (cpu !== null && cpu >= BUSY_CPU)) return "busy";
    return "ok";
}
