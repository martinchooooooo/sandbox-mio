/** Mirrors the Rust activity types (`src-tauri/src/activity/types.rs`), camelCase per serde. */

export type TaskKind =
  | "model-download"
  | "inference-setup"
  | "host-provision"
  | "agent-install"
  | "podman-startup"
  | "proxy-image-pull"
  | "network-reapply";
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

/** A point-in-time scrollback line: container lifecycle, or a synthetic terminal-task notice. */
export interface ActivityEvent {
  id: number;
  ts: number;
  text: string;
  level: "info" | "error";
}
