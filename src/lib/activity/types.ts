/** Mirrors the Rust activity types (`src-tauri/src/activity/types.rs`), camelCase per serde. */

export type TaskKind =
  | "model-download"
  | "inference-setup"
  | "host-provision"
  | "agent-install"
  | "podman-startup"
  | "proxy-image-pull"
  | "network-reapply"
  | "machine-setup"
  | "base-image-build";
export type TaskStatus = "running" | "succeeded" | "failed";

/** One long-running background operation (mirrors Rust `Task`). */
export interface Task {
  id: string;
  kind: TaskKind;
  version: number;
  status: TaskStatus;
  progress: number | null;
  target: string | null;
  detail: string | null;
  error: string | null;
  startedAtMs: number;
  endedAtMs: number | null;
}

/**
 * A summary the activity toggle spells out beside the bell, for background work the user should
 * know about without opening the panel (the onboarding setup).
 */
export interface ActivityHeadline {
  state: "running" | "done" | "attention";
  title: string;
  detail?: string;
  /** One-time coach mark above the toggle, explaining what the summary is. */
  hint?: string;
  /** Optional button inside the panel, e.g. a jump to where the problem can be fixed. */
  action?: { label: string; onclick: () => void };
}

/** A point-in-time scrollback line: container lifecycle, or a synthetic terminal-task notice. */
export interface ActivityEvent {
  id: number;
  ts: number;
  text: string;
  level: "info" | "error";
}
