import { m } from "$lib/paraglide/messages.js";

export interface Project {
    id: string;
    name: string;
    status: string;
    workspace_path: string;
    /** Absent for sandboxes predating numeric base-image versioning. */
    image_version?: number;
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
