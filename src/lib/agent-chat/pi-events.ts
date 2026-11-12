import type {
    ChatMessage,
    ChatPart,
    ChatState,
    DialogMethod,
    NoticeKind,
    PendingDialog,
    PiEffect,
    PiRecord,
    Usage,
} from "./types";

/** The adapter from pi RPC records to chat state. Deterministic and IO-free: it mutates `state`
 * in place (cheap for streaming deltas) and returns effects for the store to perform. Written
 * against pi `PI_PINNED_VERSION` (`src/lib/agents/pi.ts`). */

const DIALOG_METHODS: DialogMethod[] = ["select", "confirm", "input", "editor"];

type Obj = Record<string, unknown>;
const obj = (v: unknown): Obj => (v !== null && typeof v === "object" ? (v as Obj) : {});
const str = (v: unknown): string => (typeof v === "string" ? v : "");
const num = (v: unknown): number => (typeof v === "number" ? v : 0);

export function emptyChatState(): ChatState {
    return {
        messages: [],
        run: "idle",
        queue: { steering: [], followUp: [] },
        dialogs: [],
        usage: null,
        statuses: {},
        widgets: {},
        streaming: null,
        lastRunErrored: false,
        nextId: 0,
    };
}

function newId(state: ChatState, prefix: string): string {
    state.nextId += 1;
    return `${prefix}-${state.nextId}`;
}

/** Text of a message `content`: a plain string or the joined `text` parts. */
export function contentText(content: unknown): string {
    if (typeof content === "string") return content;
    if (!Array.isArray(content)) return "";
    return content
        .map(obj)
        .filter((p) => p.type === "text")
        .map((p) => str(p.text))
        .join("");
}

function usageOf(v: unknown): Usage | null {
    const u = obj(v);
    if (!("totalTokens" in u)) return null;
    return {
        input: num(u.input),
        output: num(u.output),
        cacheRead: num(u.cacheRead),
        cacheWrite: num(u.cacheWrite),
        totalTokens: num(u.totalTokens),
    };
}

/** The parts of an assistant message's authoritative `content`, keeping the run-time state
 * (running, output) of tool parts already shown. */
function assistantParts(content: unknown, previous: ChatPart[]): ChatPart[] {
    if (!Array.isArray(content)) return previous;
    const tools = new Map(
        previous.flatMap((p) => (p.type === "tool" ? [[p.toolCallId, p] as const] : [])),
    );
    return content.map(obj).flatMap((c): ChatPart[] => {
        switch (c.type) {
            case "text":
                return str(c.text) ? [{ type: "text", text: str(c.text), streaming: false }] : [];
            case "thinking":
                return str(c.thinking)
                    ? [{ type: "reasoning", text: str(c.thinking), streaming: false }]
                    : [];
            case "toolCall": {
                const id = str(c.id);
                const shown = tools.get(id);
                return [
                    shown ?? {
                        type: "tool",
                        toolCallId: id,
                        toolName: str(c.name),
                        input: obj(c.arguments),
                        state: "input",
                        output: "",
                    },
                ];
            }
            default:
                return [];
        }
    });
}

function errorNotice(message: Obj): ChatPart | null {
    const reason = str(message.stopReason);
    if (reason === "aborted") return { type: "notice", kind: "aborted", text: "Stopped" }; // TODO(i18n)
    if (reason === "error") {
        return { type: "notice", kind: "error", text: str(message.errorMessage) || "The model request failed" }; // TODO(i18n)
    }
    return null;
}

function findTool(state: ChatState, toolCallId: string) {
    for (let i = state.messages.length - 1; i >= 0; i--) {
        const part = state.messages[i].parts.find(
            (p) => p.type === "tool" && p.toolCallId === toolCallId,
        );
        if (part && part.type === "tool") return part;
    }
    return null;
}

/** The assistant message deltas land in, created when a delta arrives with none open (an attach
 * that began mid-message). */
function streamingMessage(state: ChatState): { message: ChatMessage; parts: Record<number, number> } {
    if (state.streaming) {
        const message = state.messages.find((m) => m.id === state.streaming!.messageId);
        if (message) return { message, parts: state.streaming.parts };
    }
    const message: ChatMessage = { id: newId(state, "a"), role: "assistant", parts: [] };
    state.messages.push(message);
    state.streaming = { messageId: message.id, parts: {} };
    return { message, parts: state.streaming.parts };
}

function pushNotice(state: ChatState, kind: NoticeKind, text: string): ChatPart {
    const part: ChatPart = { type: "notice", kind, text };
    state.messages.push({ id: newId(state, "n"), role: "notice", parts: [part] });
    return part;
}

/** The pending dialog for a raw `extension_ui_request`, or null for fire-and-forget methods. */
export function toPendingDialog(record: PiRecord): PendingDialog | null {
    const method = str(record.method) as DialogMethod;
    if (!DIALOG_METHODS.includes(method)) return null;
    return {
        id: str(record.id),
        method,
        title: str(record.title),
        message: str(record.message) || null,
        options: Array.isArray(record.options) ? record.options.map(str) : [],
        placeholder: str(record.placeholder) || null,
        prefill: str(record.prefill) || null,
    };
}

function applyUpdate(state: ChatState, record: PiRecord): void {
    const ev = obj(record.assistantMessageEvent);
    const usage = usageOf(record.usage);
    if (usage && usage.totalTokens > 0) state.usage = usage;
    const index = num(ev.contentIndex);
    const kind = str(ev.type);
    const { message, parts } = streamingMessage(state);
    const part = parts[index] === undefined ? undefined : message.parts[parts[index]];

    const open = (p: ChatPart) => {
        parts[index] = message.parts.length;
        message.parts.push(p);
        return p;
    };
    switch (kind) {
        case "text_start":
            open({ type: "text", text: "", streaming: true });
            return;
        case "thinking_start":
            open({ type: "reasoning", text: "", streaming: true });
            return;
        case "text_delta":
        case "thinking_delta": {
            const target =
                part ?? open({ type: kind === "text_delta" ? "text" : "reasoning", text: "", streaming: true });
            if (target.type === "text" || target.type === "reasoning") target.text += str(ev.delta);
            return;
        }
        case "text_end":
        case "thinking_end": {
            const target =
                part ?? open({ type: kind === "text_end" ? "text" : "reasoning", text: "", streaming: true });
            if (target.type === "text" || target.type === "reasoning") {
                target.text = str(ev.content);
                target.streaming = false;
            }
            return;
        }
        case "toolcall_end": {
            const call = obj(ev.toolCall);
            const toolCallId = str(call.id);
            if (findTool(state, toolCallId)) return;
            open({
                type: "tool",
                toolCallId,
                toolName: str(call.name),
                input: obj(call.arguments),
                state: "input",
                output: "",
            });
            return;
        }
    }
}

function applyMessage(state: ChatState, record: PiRecord, end: boolean): void {
    const message = obj(record.message);
    const role = str(message.role);
    if (role === "user" && !end) {
        state.messages.push({
            id: newId(state, "u"),
            role: "user",
            parts: [{ type: "text", text: contentText(message.content), streaming: false }],
        });
        return;
    }
    if (role !== "assistant") return;
    if (!end) {
        const created: ChatMessage = { id: newId(state, "a"), role: "assistant", parts: [] };
        state.messages.push(created);
        state.streaming = { messageId: created.id, parts: {} };
        return;
    }
    const { message: shown } = streamingMessage(state);
    shown.parts = assistantParts(message.content, shown.parts);
    const notice = errorNotice(message);
    if (notice) shown.parts.push(notice);
    state.lastRunErrored = str(message.stopReason) === "error";
    const usage = usageOf(message.usage);
    if (usage) state.usage = usage;
    state.streaming = null;
}

function toolOutput(result: unknown): string {
    return contentText(obj(result).content);
}

function applyUiRequest(state: ChatState, record: PiRecord): PiEffect[] {
    const dialog = toPendingDialog(record);
    if (dialog) {
        if (!state.dialogs.some((d) => d.id === dialog.id)) state.dialogs.push(dialog);
        return [];
    }
    switch (str(record.method)) {
        case "notify": {
            const level = str(record.notifyType);
            return [
                {
                    kind: "notify",
                    level: level === "warning" || level === "error" ? level : "info",
                    message: str(record.message),
                },
            ];
        }
        case "setStatus": {
            const key = str(record.statusKey);
            if (typeof record.statusText === "string") state.statuses[key] = record.statusText;
            else delete state.statuses[key];
            return [];
        }
        case "setWidget": {
            const key = str(record.widgetKey);
            if (Array.isArray(record.widgetLines)) state.widgets[key] = record.widgetLines.map(str);
            else delete state.widgets[key];
            return [];
        }
        case "set_editor_text":
            return [{ kind: "setEditorText", text: str(record.text) }];
        default:
            return [];
    }
}

/** Folds one pi record into `state`. `response` records are correlated by the store, not here. */
export function applyPiRecord(state: ChatState, record: PiRecord): PiEffect[] {
    switch (record.type) {
        case "agent_start":
            state.run = "running";
            state.lastRunErrored = false;
            return [];
        case "agent_settled":
            state.run = "idle";
            state.streaming = null;
            for (const m of state.messages) {
                for (const p of m.parts) if (p.type === "tool" && p.state === "running") p.state = "error";
            }
            return [{ kind: "settled", aborted: record.aborted === true }];
        case "message_start":
            applyMessage(state, record, false);
            return [];
        case "message_update":
            applyUpdate(state, record);
            return [];
        case "message_end":
            applyMessage(state, record, true);
            return [];
        case "tool_execution_start":
        case "tool_execution_update":
        case "tool_execution_end": {
            const id = str(record.toolCallId);
            let tool = findTool(state, id);
            if (!tool) {
                const { message } = streamingMessage(state);
                tool = {
                    type: "tool",
                    toolCallId: id,
                    toolName: str(record.toolName),
                    input: obj(record.args),
                    state: "running",
                    output: "",
                };
                message.parts.push(tool);
            }
            if (record.type === "tool_execution_start") {
                tool.state = "running";
                if (record.args) tool.input = obj(record.args);
            } else if (record.type === "tool_execution_update") {
                tool.output = toolOutput(record.partialResult);
            } else {
                tool.output = toolOutput(record.result);
                tool.state = record.isError === true ? "error" : "output";
            }
            return [];
        }
        case "queue_update":
            state.queue = {
                steering: Array.isArray(record.steering) ? record.steering.map(str) : [],
                followUp: Array.isArray(record.followUp) ? record.followUp.map(str) : [],
            };
            return [];
        case "compaction_start":
            pushNotice(state, "compaction", "Compacting the conversation to fit the context window…"); // TODO(i18n)
            return [];
        case "compaction_end": {
            const result = obj(record.result);
            const text =
                record.aborted === true
                    ? "Compaction was cancelled" // TODO(i18n)
                    : record.result
                      ? `Compacted ${num(result.tokensBefore).toLocaleString()} tokens into a summary` // TODO(i18n)
                      : `Compaction failed: ${str(record.errorMessage)}`; // TODO(i18n)
            pushNotice(state, record.result ? "compaction" : "error", text);
            return [];
        }
        case "auto_retry_start":
            pushNotice(
                state,
                "retry",
                `Retrying (attempt ${num(record.attempt)} of ${num(record.maxAttempts)}): ${str(record.errorMessage)}`, // TODO(i18n)
            );
            return [];
        case "auto_retry_end":
            if (record.success !== true) {
                pushNotice(state, "error", `Retries exhausted: ${str(record.finalError)}`); // TODO(i18n)
            }
            return [];
        case "extension_error":
            pushNotice(state, "error", `Extension error: ${str(record.error)}`); // TODO(i18n)
            return [];
        case "extension_ui_request":
            return applyUiRequest(state, record);
        default:
            return [];
    }
}

/** Removes an answered dialog. */
export function resolveDialog(state: ChatState, id: string): void {
    state.dialogs = state.dialogs.filter((d) => d.id !== id);
}

/** Rebuilds the message list from pi's `get_messages` history (a fresh attach). */
export function messagesFromHistory(state: ChatState, history: unknown[]): ChatMessage[] {
    const messages: ChatMessage[] = [];
    for (const raw of history.map(obj)) {
        const role = str(raw.role);
        if (role === "user") {
            messages.push({
                id: newId(state, "u"),
                role: "user",
                parts: [{ type: "text", text: contentText(raw.content), streaming: false }],
            });
        } else if (role === "assistant") {
            const parts = assistantParts(raw.content, []);
            const notice = errorNotice(raw);
            if (notice) parts.push(notice);
            messages.push({ id: newId(state, "a"), role: "assistant", parts });
        } else if (role === "toolResult") {
            const id = str(raw.toolCallId);
            for (let i = messages.length - 1; i >= 0; i--) {
                const tool = messages[i].parts.find((p) => p.type === "tool" && p.toolCallId === id);
                if (tool && tool.type === "tool") {
                    tool.output = contentText(raw.content);
                    tool.state = raw.isError === true ? "error" : "output";
                    break;
                }
            }
        }
    }
    return messages;
}
