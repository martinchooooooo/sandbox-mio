import { invoke } from "@tauri-apps/api/core";

// Module-scoped per-sandbox cache: WorkspaceDashboard, PackageManager and AgentManager all need the
// installed-package list for the same sandbox, so it lives here once instead of three independent
// local copies that could drift out of sync after an install/uninstall.

const packagesBySandbox = $state<Record<string, string[]>>({});
const inFlight = new Map<string, Promise<void>>();

export function packagesFor(sandboxName: string): string[] {
    return packagesBySandbox[sandboxName] ?? [];
}

/** Refetch `sandboxName`'s package list from Rust and update the shared cache. */
export async function refreshPackages(sandboxName: string, workspacePath: string): Promise<void> {
    packagesBySandbox[sandboxName] = await invoke<string[]>("list_packages", { workspacePath });
}

/** Fetch `sandboxName`'s packages if not already cached or loading; de-dupes concurrent callers
 * (WorkspaceDashboard, PackageManager and AgentManager all mount together for the same sandbox). */
export function ensurePackages(sandboxName: string, workspacePath: string): void {
    if (!workspacePath || sandboxName in packagesBySandbox || inFlight.has(sandboxName)) return;
    const promise = refreshPackages(sandboxName, workspacePath).finally(() => inFlight.delete(sandboxName));
    inFlight.set(sandboxName, promise);
}
