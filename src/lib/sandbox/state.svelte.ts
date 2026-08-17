import { invoke } from "@tauri-apps/api/core";
import { checkPodman, podmanUnavailable } from "$lib/podman/setup.svelte";
import { parseError } from "$lib/errors";
import type { Project, Deployment } from "$lib/sandbox";
import { allCapabilities, type ProviderCapabilities, type ProviderId } from "$lib/providers";

// Module-scoped store (mirrors src/lib/workspace/state.svelte.ts): the shared sandbox-list,
// deployment, host, and capability data read by both the projects list
// (src/routes/projects/+page.svelte) and the per-sandbox admin controls relocated into
// WorkspaceDashboard.svelte (RemoteManager.svelte). Both surfaces render as siblings, not
// ancestor/descendant (docs/features/consolidate-app-container/02_dev.md), so a Destroy or
// Deploy/Undeploy/Sync triggered from the Dashboard has no direct prop path back to the list —
// this store is that path.

let projects = $state<Project[]>([]);
let deployments = $state<Record<string, Deployment>>({});
let hosts = $state<string[]>([]);
let capabilities = $state<Record<ProviderId, ProviderCapabilities> | undefined>(undefined);
let loadError = $state("");

let started = false;
let pollId: ReturnType<typeof setInterval> | undefined;

async function loadProjects() {
    try {
        projects = await invoke<Project[]>("list_sandboxes");
        loadError = "";
    } catch (e) {
        loadError = parseError(e).message;
    }
}

async function loadDeployments() {
    try {
        const list = await invoke<Deployment[]>("sandbox_deployments");
        deployments = Object.fromEntries(list.map((d) => [d.name, d]));
    } catch (e) {
        loadError = parseError(e).message;
    }
}

async function loadHosts() {
    try {
        hosts = await invoke<string[]>("list_sandbox_hosts");
    } catch (e) {
        loadError = parseError(e).message;
    }
}

async function loadCapabilities() {
    try {
        capabilities = await allCapabilities();
    } catch (e) {
        loadError = parseError(e).message;
    }
}

export function sandboxesList(): Project[] {
    return projects;
}

export function deploymentFor(name: string): Deployment | undefined {
    return deployments[name];
}

export function hostsList(): string[] {
    return hosts;
}

export function capabilitiesFor(provider: ProviderId): ProviderCapabilities | undefined {
    return capabilities?.[provider];
}

export function sandboxLoadError(): string {
    return loadError;
}

// Re-fetches the list and deployment overlay immediately rather than waiting out the poll
// interval — called after a mutating action (start/stop/deploy/undeploy/sync/destroy) so the UI
// reflects it without a stale window. Every one of those actions can change either list, so both
// reload together rather than the caller having to know which.
export async function refreshSandboxState(): Promise<void> {
    await Promise.all([loadProjects(), loadDeployments()]);
}

// Hydrates and keeps sandbox-list/deployment/host/capability data live via a 3 s poll, gated
// behind Podman readiness so a down Podman is never flooded with failing invocations. Callers
// (the projects list, WorkspaceDashboard) may not have already probed Podman themselves, so this
// takes that gating responsibility itself rather than trusting the caller did — idempotent and
// safe to call from any component.
export async function ensureSandboxRemoteState(): Promise<void> {
    if (started) return;
    started = true;
    try {
        await checkPodman();
    } catch {
        started = false;
        return;
    }
    if (podmanUnavailable()) {
        started = false;
        return;
    }
    loadHosts();
    loadCapabilities();
    await refreshSandboxState();
    pollId = setInterval(refreshSandboxState, 3000);
}
