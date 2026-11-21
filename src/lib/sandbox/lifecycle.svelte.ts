import { invoke } from "@tauri-apps/api/core";
import { toast } from "svelte-sonner";
import { m } from "$lib/paraglide/messages.js";
import { parseError } from "$lib/errors";
import { refreshSandboxState, sandboxesList } from "$lib/sandbox/state.svelte";
import { tasks } from "$lib/activity/activity.svelte";
import { openSandbox } from "$lib/workspace/navigation.svelte";
import type { Project } from "$lib/sandbox";
import type { ProviderId } from "$lib/providers";
import { installAgentInSandbox } from "$lib/agents/install";
import { agentsById, planPreinstall, readPreinstallFacts } from "$lib/agents/preinstall";

// Module-scoped store (mirrors src/lib/sandbox/state.svelte.ts): the in-flight lifecycle operation
// and last failure per sandbox name. Component-local runners lost both on navigation, so a start
// from a Home card showed nothing in the sidebar or header and could be fired twice. Every surface
// (Home card, header, sidebar) reads this instead.

export type LifecycleOp = "create" | "start" | "stop" | "restart" | "destroy";

let pending = $state<Record<string, LifecycleOp>>({});
let errors = $state<Record<string, string>>({});

export function pendingOp(name: string): LifecycleOp | undefined {
    return pending[name];
}

export function lifecycleError(name: string): string {
    return errors[name] ?? "";
}

export function clearLifecycleError(name: string): void {
    delete errors[name];
}

export function lifecycleMessage(op: LifecycleOp): string {
    switch (op) {
        case "start": return m.projects_status_starting();
        case "stop": return m.projects_status_stopping();
        case "restart": return "Restarting…"; // TODO(i18n)
        case "create": return "Creating…"; // TODO(i18n)
        case "destroy": return "Destroying…"; // TODO(i18n)
    }
}

/**
 * Runs `work` as the sandbox's one pending operation. A no-op returning false while another is
 * pending for the same name. A failure is stored and toasted, because the user may have navigated
 * away from every surface that shows the stored error.
 */
async function runLifecycle(name: string, op: LifecycleOp, work: () => Promise<void>): Promise<boolean> {
    if (pending[name] !== undefined) return false;
    pending[name] = op;
    delete errors[name];
    try {
        await work();
        await refreshSandboxState();
        return true;
    } catch (e) {
        const message = parseError(e).message;
        errors[name] = message;
        toast.error(`${name}: ${message}`);
        return false;
    } finally {
        delete pending[name];
    }
}

export function startSandbox(name: string): Promise<boolean> {
    return runLifecycle(name, "start", () => invoke("start_sandbox", { name }));
}

export function stopSandbox(name: string): Promise<boolean> {
    return runLifecycle(name, "stop", () => invoke("stop_sandbox", { name }));
}

// Stop then Start, never a backend restart, so a stop's pane teardown still runs
// (docs/features/sandbox-resource-monitor/02_dev.md, Resolved Decision 12). Pending stays "restart"
// through both steps.
export function restartSandbox(name: string): Promise<boolean> {
    return runLifecycle(name, "restart", async () => {
        await invoke("stop_sandbox", { name });
        await invoke("start_sandbox", { name });
    });
}

/**
 * Destroys the sandbox. The backend guard refuses to discard unsynced remote work; `confirmForce`
 * is then asked with the guard's message and the destroy is retried with force only on true.
 * Resolves true only when the sandbox was actually destroyed: a declined confirmation resolves
 * false without an error or toast. The confirmation runs while the op is pending, because the
 * guard's refusal is only known from inside the invoke.
 */
export async function destroySandbox(
    name: string,
    opts: { confirmForce: (message: string) => boolean },
): Promise<boolean> {
    let declined = false;
    const ok = await runLifecycle(name, "destroy", async () => {
        try {
            await invoke("destroy_sandbox", { name, force: false });
        } catch (e) {
            const err = parseError(e);
            if (err.kind !== "UnsyncedChanges") throw e;
            if (!opts.confirmForce(err.message)) {
                declined = true;
                return;
            }
            await invoke("destroy_sandbox", { name, force: true });
        }
    });
    return ok && !declined;
}

/** Arguments of `create_sandbox`, kept on the placeholder so a failed create can be retried. */
export interface CreateArgs {
    name: string;
    existingPath?: string;
    /** Omitted means the backend resolves the default provider. */
    provider?: ProviderId;
    proxy: boolean;
    /** Present only when the provider supports an egress policy. */
    network?: { egress: "open" | "allowlist"; starterHosts: string[] };
    /** Agent ids to install after create; omitted means plan from host state (`planPreinstall`). */
    agents?: string[];
    /** False leaves opening the new sandbox to the caller; omitted means open it once created. */
    autoOpen?: boolean;
    /** Called as soon as the sandbox exists, before any agent install. */
    onCreated?: () => void;
    /** Called once the agent installs ended, with the ids that installed successfully. */
    onInstalled?: (installed: string[]) => void;
}

export interface PendingCreate {
    name: string;
    /** Current step text while creating. */
    step: string;
    /** Non-empty once the create failed; the placeholder then waits for Retry or Dismiss. */
    error: string;
}

// A create can take minutes and the sandbox is not listed until it returns, so each create is held
// here as a placeholder that the sidebar and Home render beside the real sandboxes.
let creates = $state<Record<string, { args: CreateArgs; error: string }>>({});

export function pendingCreates(): PendingCreate[] {
    return Object.entries(creates).map(([name, c]) => ({
        name,
        step: createStep(name),
        error: c.error,
    }));
}

// Install progress the backend cannot report: it knows only the create task, not the agent install after it.
let installNotes = $state<Record<string, string>>({});

// The backend reports each step as the `detail` of the running `sandbox-create` task for this name.
function createStep(name: string): string {
    if (installNotes[name]) return installNotes[name];
    const task = tasks().find((t) => t.kind === "sandbox-create" && t.target === name && t.status === "running");
    return task?.detail || lifecycleMessage("create");
}

/**
 * Starts a create in the background. Returns false, starting nothing, when the name is already
 * pending or already listed. Failures stay on the placeholder and are not toasted here: the
 * activity store toasts a failed create task, and a name or validation error that never opened a
 * task shows inline on the placeholder.
 */
export function createSandbox(args: CreateArgs): boolean {
    const { name } = args;
    if (pending[name] !== undefined || creates[name] !== undefined) return false;
    if (sandboxesList().some((p) => p.name === name)) return false;
    creates[name] = { args, error: "" };
    void runCreate(name);
    return true;
}

export function retryCreate(name: string): void {
    const entry = creates[name];
    if (!entry || !entry.error || pending[name] !== undefined) return;
    entry.error = "";
    void runCreate(name);
}

export function dismissCreate(name: string): void {
    if (creates[name]?.error) delete creates[name];
}

/**
 * Best-effort: a failed install is toasted and never fails the create, since the Manage tab still
 * offers the manual install (docs/features/fastest-time-to-wow/02_dev.md, Phase D).
 */
async function preinstallAgents(name: string, workspacePath: string, explicit: string[] | undefined): Promise<string[]> {
    const installed: string[] = [];
    try {
        const ids = explicit ?? planPreinstall(await readPreinstallFacts());
        for (const agent of agentsById(ids)) {
            installNotes[name] = m.projects_preinstall_installing({ agent: agent.label });
            try {
                await installAgentInSandbox(name, workspacePath, agent);
                installed.push(agent.id);
            } catch (e) {
                toast.error(m.projects_preinstall_failed({ name, agent: agent.label, message: parseError(e).message }));
            }
        }
    } finally {
        delete installNotes[name];
    }
    return installed;
}

async function runCreate(name: string): Promise<void> {
    const { args } = creates[name];
    pending[name] = "create";
    try {
        const sandbox = await invoke<Project>("create_sandbox", {
            name,
            existingPath: args.existingPath,
            provider: args.provider,
            proxy: args.proxy,
            ...args.network,
        });
        args.onCreated?.();
        const installed = await preinstallAgents(name, sandbox.workspace_path, args.agents);
        args.onInstalled?.(installed);
        await refreshSandboxState();
        // The placeholder goes only once the real entry is listed, so there is no gap or flicker.
        delete creates[name];
        delete pending[name];
        if (args.autoOpen === false) return;
        try {
            await openSandbox(name, sandbox.workspace_path, { restore: false });
        } catch (e) {
            toast.error(`${name}: ${parseError(e).message}`);
        }
    } catch (e) {
        if (creates[name]) creates[name].error = parseError(e).message;
    } finally {
        delete pending[name];
    }
}
