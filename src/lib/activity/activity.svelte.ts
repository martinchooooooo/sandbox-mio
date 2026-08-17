import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { toast } from "svelte-sonner";

import { m } from "$lib/paraglide/messages.js";
import type { ActivityEvent, Task, TaskKind, TaskStatus } from "./types";

// Module-scoped store (mirrors src/lib/registry.svelte.ts): the `$state` lives above the router,
// so it survives every route change. `ensureActivity()` is called once from the root layout.

// The authoritative task ledger, folded by id. The Rust `TaskRegistry` is the source of truth;
// this is a cache hydrated from `list_tasks` and kept live by `activity://task-updated`.
let tasksById = $state<Record<string, Task>>({});
// Append-only scrollback (container lifecycle + synthetic terminal-task lines), capped FIFO.
let events = $state<ActivityEvent[]>([]);
// Failures the user has not yet seen (cleared by markSeen / opening the panel).
let unseenFailures = $state(0);
// Latest container status per sandbox name, derived from the listen_events stream. The projects
// page reads this instead of mounting its own EventLog.
let containerStatus = $state<Record<string, string>>({});

let started = false;
let eventSeq = 0;

const EVENT_CAP = 500;

// Container action → sandbox status (moved verbatim from the removed EventLog.svelte).
const ACTION_STATUS: Record<string, string> = {
    start: "running",
    unpause: "running",
    stop: "exited",
    die: "exited",
    died: "exited",
    kill: "exited",
    pause: "paused",
    create: "created",
    destroy: "removed",
};

// Explicit map required for Paraglide; missing kinds render blank (guarded by e2e).
const kindLabels: Record<TaskKind, () => string> = {
    "model-download": m.task_model_download_label,
    "inference-setup": m.task_inference_setup_label,
    "host-provision": m.task_host_provision_label,
    "agent-install": m.task_agent_install_label,
    "podman-startup": m.task_podman_startup_label,
    // TODO(i18n)
    "proxy-image-pull": () => "Pulling proxy image",
};

export function taskLabel(kind: TaskKind): string {
    return kindLabels[kind]?.() ?? "";
}

/** The ledger as a list: running tasks first, then by most-recently-started. */
export function tasks(): Task[] {
    return Object.values(tasksById).sort((a, b) => {
        const ar = a.status === "running" ? 0 : 1;
        const br = b.status === "running" ? 0 : 1;
        return ar - br || b.startedAtMs - a.startedAtMs;
    });
}

export function activityEvents(): ActivityEvent[] {
    return events;
}

/** Running tasks plus unseen failures — the badge number. */
export function badgeCount(): number {
    const running = Object.values(tasksById).filter((t) => t.status === "running").length;
    return running + unseenFailures;
}

/** Zero the unseen-failure count (called when the panel is opened). */
export function markSeen(): void {
    unseenFailures = 0;
}

/** Latest container status for a sandbox name, or undefined if none seen yet. */
export function containerStatusFor(name: string): string | undefined {
    return containerStatus[name];
}

function pushEvent(text: string, level: "info" | "error"): void {
    const ev: ActivityEvent = { id: eventSeq++, ts: Date.now(), text, level };
    events = [...events, ev].slice(-EVENT_CAP);
}

function onTransition(prev: Task | undefined, next: Task): void {
    if (!next.status || next.status === "running") return;
    if (prev?.status === next.status) return; // already announced this terminal state
    const label = taskLabel(next.kind);
    const subject = next.target ? `${label}: ${next.target}` : label;
    if (next.status === "succeeded") {
        toast.success(m.activity_toast_succeeded({ label: subject }));
        pushEvent(m.activity_toast_succeeded({ label: subject }), "info");
    } else if (next.status === "failed") {
        toast.error(m.activity_toast_failed({ label: subject }), { duration: Infinity });
        pushEvent(`${m.activity_toast_failed({ label: subject })}${next.error ? ` — ${next.error}` : ""}`, "error");
        unseenFailures += 1;
    }
}

// The strictly-greater-version guard is the race fix: it makes the `list_tasks` snapshot and the
// live `activity://task-updated` stream commutative, so neither clobbers the other (Protocol B).
function fold(t: Task): void {
    const cur = tasksById[t.id];
    if (cur && t.version <= cur.version) return;
    tasksById[t.id] = t;
    onTransition(cur, t);
}

// Fold a raw Podman container lifecycle event from listen_events into the scrollback + status map.
// Mirrors the defensive key-probing the removed EventLog.svelte did (the events arrive untyped).
function pushContainerEvent(ev: Record<string, unknown>): void {
    const type = String((ev.Type ?? ev.type) ?? "");
    const action = String((ev.Action ?? ev.action ?? ev.Status ?? ev.status) ?? "");
    const attrs = (ev.Actor as Record<string, Record<string, string>>)?.Attributes
        ?? (ev.actor as Record<string, Record<string, string>>)?.attributes
        ?? {};
    const name = String(attrs.name ?? (ev.Name as string) ?? (ev.name as string) ?? ev.id ?? "");
    const time = new Date().toLocaleTimeString();
    pushEvent(`[${time}] ${type} ${action}${name ? ` — ${name}` : ""}`, "info");
    if (type === "container" && name && action in ACTION_STATUS) {
        containerStatus[name] = ACTION_STATUS[action];
    }
}

/**
 * Hydrate the store and keep it live. Protocol B order (proven by the 2026-06-29 race spike):
 * subscribe to the live event *before* reading the snapshot, and fold both through the version
 * guard, so an emit landing in the snapshot gap is never lost. Idempotent (guarded by `started`).
 */
export async function ensureActivity(): Promise<void> {
    if (started) return;
    started = true;
    try {
        // 1. Subscribe first.
        await listen<Task>("activity://task-updated", (e) => fold(e.payload));
        // 2. Then hydrate from the registry snapshot.
        const snapshot = await invoke<Task[]>("list_tasks");
        for (const t of snapshot) fold(t);
        // 3. Start the container event stream (lifted from the removed EventLog.svelte).
        const ch = new Channel<Record<string, unknown>>();
        ch.onmessage = pushContainerEvent;
        await invoke("listen_events", { onEvent: ch });
    } catch {
        // Best-effort: a failed hydration must not crash the layout. The next mutation's live
        // event still folds in. Reset the guard so a later caller can retry.
        started = false;
    }
}
