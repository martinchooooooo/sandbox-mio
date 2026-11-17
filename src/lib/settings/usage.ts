import type { ProviderId } from "$lib/providers";
import { STRAINED_MEMORY } from "$lib/sandbox/health";

// The sizing report's arithmetic over `get_resource_history` rollups
// (docs/features/sandbox-resource-monitor/02_dev.md, Usage history and the sizing report).

/** Mirrors Rust `services::resources::Rollup`; memory in MiB, absent CPU fields omitted. */
export interface Rollup {
    ts: number;
    provider: ProviderId;
    cpu?: number;
    cpuMax?: number;
    mem: number;
    memMax: number;
    limit: number;
    high: number;
    includesCache?: boolean;
}

/** Mirrors Rust `services::resources::PoolRollup`; memory in MiB. */
export interface PoolRollup {
    ts: number;
    cpus: number;
    total: number;
    free: number;
}

const MIB = 1024 * 1024;

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
    const peaks = rollups.map((r) => r.memMax * MIB);
    const cpu = rollups.flatMap((r) => (r.cpu === undefined ? [] : [r.cpu]));
    return {
        name,
        provider: latest.provider,
        memoryLimit: latest.limit * MIB,
        peakMemory: Math.max(...peaks),
        p95Memory: percentile(peaks, 95),
        minutesHigh: rollups.filter((r) => r.high > 0).length,
        meanCpu: cpu.length === 0 ? null : cpu.reduce((sum, c) => sum + c, 0) / cpu.length,
        minutes: rollups.length,
    };
}

/** The Podman machine's row. Its "used" is total minus free, which counts page cache as used. */
export function poolRow(name: string, pool: PoolRollup[]): UsageRow | null {
    if (pool.length === 0) return null;
    const used = pool.map((p) => (p.total - p.free) * MIB);
    const latest = pool.reduce((a, b) => (b.ts > a.ts ? b : a));
    return {
        name,
        provider: null,
        memoryLimit: latest.total * MIB,
        peakMemory: Math.max(...used),
        p95Memory: percentile(used, 95),
        minutesHigh: pool.filter((p) => p.total - p.free >= STRAINED_MEMORY * p.total).length,
        meanCpu: null,
        minutes: pool.length,
    };
}
