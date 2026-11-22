import type { ToolKind } from "$lib/story-stage";
import type { ChatPart, ChatState, ToolChatPart } from "./types";

/** Which half of an agent a part belongs to: the Brain (the model) thinks, replies and asks for
 * tools; the Arms run them in the sandbox. */
export type Role = "brain-think" | "brain-reply" | "brain-ask" | "arms-act";

export type BeatKind = "idle" | "thinking" | "asking" | "acting" | "returning" | "replying" | "retrying";

/** What the agent is doing right now; the tool fields name every tool part involved. */
export interface Beat {
    kind: BeatKind;
    toolCallIds: string[];
    toolKinds: (ToolKind | null)[];
}

const requested = (t: ToolChatPart) => t.state === "composing" || t.state === "input";

/** The roles a part plays: a tool part is the Brain's request, plus the Arms' execution once it runs. */
export function rolesOf(part: ChatPart): Role[] {
    switch (part.type) {
        case "reasoning":
            return ["brain-think"];
        case "text":
            return ["brain-reply"];
        case "tool":
            return requested(part) ? ["brain-ask"] : ["brain-ask", "arms-act"];
        default:
            return [];
    }
}

/** The stage tile for a pi tool name; null draws no specific tile. */
export function toolKindOf(toolName: string): ToolKind | null {
    switch (toolName) {
        case "bash":
            return "terminal";
        case "read":
            return "read";
        case "edit":
        case "write":
            return "edit";
        case "grep":
        case "find":
        case "ls":
            return "find";
        default:
            return null;
    }
}

function beat(kind: BeatKind, tools: ToolChatPart[] = []): Beat {
    return {
        kind,
        toolCallIds: tools.map((t) => t.toolCallId),
        toolKinds: tools.map((t) => toolKindOf(t.toolName)),
    };
}

/** True when two beats would draw the same thing. */
export function sameBeat(a: Beat, b: Beat): boolean {
    return a.kind === b.kind && a.toolCallIds.join() === b.toolCallIds.join();
}

/** Reduces the newest assistant message and the run state to one beat. Reads the newest opened
 * part, never the `streaming` flags: pi keeps a reasoning block open until the message ends. */
export function currentBeat(chat: Pick<ChatState, "messages" | "run">): Beat {
    if (chat.run !== "running") return beat("idle");
    const latest = chat.messages.findLast((m) => m.role !== "notice");
    if (!latest || latest.role === "user") return beat("thinking");

    const last = latest.parts.at(-1);
    if (last?.type === "notice" && last.kind === "error") return beat("retrying");
    const newest = latest.parts.findLast((p) => p.type !== "notice");
    if (!newest) return beat("thinking");
    if (newest.type === "reasoning") return beat("thinking");
    if (newest.type === "text") return beat("replying");

    const tools = latest.parts.filter((p): p is ToolChatPart => p.type === "tool");
    // Acting keeps every started call until the last one ends, so parallel calls hold one beat.
    if (tools.some((t) => t.state === "running")) return beat("acting", tools.filter((t) => !requested(t)));
    const asked = tools.filter(requested);
    if (asked.length > 0) return beat("asking", asked);
    return beat("returning", tools);
}
