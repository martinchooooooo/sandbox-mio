import { m } from "$lib/paraglide/messages.js";
import type { ProviderId } from "$lib/providers";

export interface Project {
    id: string;
    name: string;
    status: string;
    workspace_path: string;
    /** Absent for sandboxes predating numeric base-image versioning. */
    image_version?: number;
    /** Which engine runs it — what gates the actions its provider cannot do (deploy). */
    provider: ProviderId;
}

export type SyncStatus = "syncing" | "up-to-date" | "conflict" | "error";

export interface Deployment {
    name: string;
    host: string;
    status: SyncStatus;
}

// Maps container and sync status strings returned by the Rust backend to translation keys. When the
// backend introduces a new status value, add a matching `status_<value>` key to every locale file.
export function translateStatus(status: string): string {
    const map: Record<string, () => string> = {
        running:     m.status_running,
        paused:      m.status_paused,
        exited:      m.status_exited,
        created:     m.status_created,
        removed:     m.status_removed,
        syncing:     m.status_syncing,
        "up-to-date": m.status_up_to_date,
        conflict:    m.status_conflict,
        error:       m.status_error,
    };
    return map[status]?.() ?? status;
}

// Colour for a deployed sandbox's sync-status dot, mirroring the container-state dot idiom.
export function syncColor(status: SyncStatus): string {
    return status === "up-to-date" ? "bg-green-500" :
           status === "syncing"    ? "bg-blue-400" :
           status === "conflict"   ? "bg-yellow-400" :
           "bg-red-500";
}

/** Status-dot colour for a sandbox: its sync status when deployed, else its container state. */
export function statusDotClass(status: string | undefined, deployment: Deployment | undefined): string {
    if (deployment) return syncColor(deployment.status);
    return status === "running" ? "bg-green-500" :
           status === "paused"  ? "bg-yellow-400" :
           status === "exited"  ? "bg-red-400" :
           "bg-muted-foreground/40";
}

const BYTE_UNITS = ["B", "KB", "MB", "GB", "TB"];

/** A byte count in binary units with at most one decimal, e.g. `1.2 GB`. */
export function formatBytes(bytes: number): string {
    let value = bytes;
    let unit = 0;
    while (value >= 1024 && unit < BYTE_UNITS.length - 1) {
        value /= 1024;
        unit++;
    }
    const digits = unit === 0 || value >= 100 ? 0 : 1;
    return `${Number(value.toFixed(digits))} ${BYTE_UNITS[unit]}`;
}

/** A CPU fraction (0 to 1) as a whole percentage, e.g. `12 %`. */
export function formatCpu(fraction: number): string {
    return `${Math.round(fraction * 100)} %`;
}
