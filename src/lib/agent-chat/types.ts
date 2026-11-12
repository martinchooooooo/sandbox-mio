/** Local message-and-parts shapes the chat renders, structurally compatible with what the AI
 * Elements patterns read (`docs/features/native-agent-ui/02_dev.md`, Resolved Decision 9). */

/** Mirrors Rust `Cursor` (`src-tauri/src/agent_host/types.rs`). */
export interface Cursor {
    generation: string;
    offset: number;
}

/** Mirrors Rust `HostStatus`. */
export interface HostStatus {
    generation: string;
    piVersion: string;
    started: boolean;
    pendingDialogs: PiRecord[];
}

/** Mirrors Rust `AttachInfo`. */
export interface AttachInfo {
    generation: string;
    offset: number;
    writer: boolean;
}

/** Mirrors Rust `AttachResult` (`src-tauri/src/commands/agent_chat.rs`). */
export interface AttachResult {
    attach: AttachInfo;
    status: HostStatus;
}

/** One journal record and its byte span; both offsets are null on the synthetic `mio_detached`. */
export interface AttachEvent {
    offset: number | null;
    end: number | null;
    record: PiRecord;
}

/** A raw pi RPC record. Fields are read defensively by `pi-events.ts`. */
export type PiRecord = { type: string; [key: string]: unknown };

/** Mirrors Rust `ChatSession` (`src-tauri/src/agent_host/sessions.rs`). */
export interface ChatSession {
    id: string;
    sandboxPath: string;
    name: string | null;
    firstMessage: string | null;
    messageCount: number;
    modifiedMs: number;
}

export type ToolState = "input" | "running" | "output" | "error";

export type ChatPart =
    | { type: "text"; text: string; streaming: boolean }
    | { type: "reasoning"; text: string; streaming: boolean }
    | {
          type: "tool";
          toolCallId: string;
          toolName: string;
          input: Record<string, unknown>;
          state: ToolState;
          output: string;
      }
    | { type: "notice"; kind: NoticeKind; text: string };

export type NoticeKind = "compaction" | "retry" | "error" | "aborted";

export interface ChatMessage {
    id: string;
    role: "user" | "assistant" | "notice";
    parts: ChatPart[];
}

export type DialogMethod = "select" | "confirm" | "input" | "editor";

/** A blocking `extension_ui_request` pi waits on until it is answered by id. */
export interface PendingDialog {
    id: string;
    method: DialogMethod;
    title: string;
    message: string | null;
    options: string[];
    placeholder: string | null;
    prefill: string | null;
}

/** "idle" only after `agent_settled`, never after `agent_end` (Common Gotchas). */
export type RunState = "idle" | "running";

export interface Usage {
    input: number;
    output: number;
    cacheRead: number;
    cacheWrite: number;
    totalTokens: number;
}

export interface QueueState {
    steering: string[];
    followUp: string[];
}

/** Everything `applyPiRecord` folds pi's records into. */
export interface ChatState {
    messages: ChatMessage[];
    run: RunState;
    queue: QueueState;
    dialogs: PendingDialog[];
    usage: Usage | null;
    /** `setStatus` entries by key. */
    statuses: Record<string, string>;
    /** `setWidget` lines by key. */
    widgets: Record<string, string[]>;
    /** Where streaming deltas land: the assistant message and its parts by `contentIndex`. */
    streaming: { messageId: string; parts: Record<number, number> } | null;
    /** Whether the last settled run ended on a provider error, for the inference-down check. */
    lastRunErrored: boolean;
    nextId: number;
}

/** Side effects a record asks the store to perform; the adapter itself does no IO. */
export type PiEffect =
    | { kind: "notify"; level: "info" | "warning" | "error"; message: string }
    | { kind: "setEditorText"; text: string }
    | { kind: "settled"; aborted: boolean };
