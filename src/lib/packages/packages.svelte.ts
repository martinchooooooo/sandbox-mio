import { listPackages } from "$lib/api/packages";
import { untrack } from "svelte";

// Module-scoped per-sandbox cache to avoid drifting independent copies across components.

const packagesBySandbox = $state<Record<string, string[]>>({});
const inFlight = new Map<string, Promise<void>>();

export function packagesFor(sandboxName: string): string[] {
    return packagesBySandbox[sandboxName] ?? [];
}

/** Refetch `sandboxName`'s package list from Rust and update the shared cache. */
export async function refreshPackages(sandboxName: string, workspacePath: string): Promise<void> {
    packagesBySandbox[sandboxName] = await listPackages(workspacePath);
}

/** Fetch `sandboxName`'s packages if not already cached or loading; de-dupes concurrent callers
 * (WorkspaceDashboard, the Overview's AgentManager and the Tools section's PackageManager each call it). */
export function ensurePackages(sandboxName: string, workspacePath: string): void {
    // Untracked so a caller's `$effect` depends only on its arguments, not on the cache it checks.
    untrack(() => {
        if (!workspacePath || sandboxName in packagesBySandbox || inFlight.has(sandboxName)) return;
        const promise = refreshPackages(sandboxName, workspacePath).finally(() => inFlight.delete(sandboxName));
        inFlight.set(sandboxName, promise);
    });
}
