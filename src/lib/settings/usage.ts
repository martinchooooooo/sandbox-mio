import type { ProviderId } from "$lib/providers";
import { STRAINED_MEMORY } from "$lib/sandbox/health";

// The sizing report's arithmetic over `get_resource_history` rollups
// (docs/features/sandbox-resource-monitor/02_dev.md, Usage history and the sizing report).

/** Mirrors Rust `services::resources::Rollup`. */
export interface Rollup {
    ts: number;
    provider: ProviderId;
    cpuMean: number | null;
    cpuMax: number | null;
    memMean: number;
    memMax: number;
    memLimit: number;
    samples: number;
    samplesHigh: number;
    memIncludesCache: boolean;
}

/** Mirrors Rust `services::resources::PoolRollup`. */
export interface PoolRollup {
    ts: number;
    cpus: number;
    memoryTotal: number;
    memoryFree: number;
}

/** Mirrors Rust `services::resources::ResourceHistory`. */
export interface ResourceHistory {
    sandboxes: { name: string; rollups: Rollup[] }[];
    podmanPool: PoolRollup[];
}

export interface UsageRow {
    name: string;
    /** Null for the Podman machine row. */
    provider: ProviderId | null;
    memoryLimit: number;
    peakMemory: number;
    p95Memory: number;
    /** Minutes with at least one sample at or above `STRAINED_MEMORY` of the limit. */
    minutesHigh: number;
    /** Null when no CPU reading exists (the Podman machine row). */
    meanCpu: number | null;
    minutes: number;
}

/** Nearest-rank percentile; 0 for an empty list. */
export function percentile(values: number[], p: number): number {
    if (values.length === 0) return 0;
    const sorted = [...values].sort((a, b) => a - b);
    const rank = Math.ceil((p / 100) * sorted.length);
    return sorted[Math.min(sorted.length, Math.max(1, rank)) - 1];
}

export function sandboxRow(name: string, rollups: Rollup[]): UsageRow | null {
    if (rollups.length === 0) return null;
    const latest = rollups.reduce((a, b) => (b.ts > a.ts ? b : a));
    const peaks = rollups.map((r) => r.memMax);
    const cpu = rollups.filter((r) => r.cpuMean !== null);
    return {
        name,
        provider: latest.provider,
        memoryLimit: latest.memLimit,
        peakMemory: Math.max(...peaks),
        p95Memory: percentile(peaks, 95),
        minutesHigh: rollups.filter((r) => r.samplesHigh > 0).length,
        meanCpu: cpu.length === 0 ? null : cpu.reduce((sum, r) => sum + (r.cpuMean ?? 0), 0) / cpu.length,
        minutes: rollups.length,
    };
}

/** The Podman machine's row. Its "used" is total minus free, which counts page cache as used. */
export function poolRow(name: string, pool: PoolRollup[]): UsageRow | null {
    if (pool.length === 0) return null;
    const used = pool.map((p) => p.memoryTotal - p.memoryFree);
    const latest = pool.reduce((a, b) => (b.ts > a.ts ? b : a));
    return {
        name,
        provider: null,
        memoryLimit: latest.memoryTotal,
        peakMemory: Math.max(...used),
        p95Memory: percentile(used, 95),
        minutesHigh: pool.filter((p) => p.memoryTotal - p.memoryFree >= STRAINED_MEMORY * p.memoryTotal).length,
        meanCpu: null,
        minutes: pool.length,
    };
}
