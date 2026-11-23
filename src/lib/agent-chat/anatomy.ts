import type { ChatPart, RunState, ToolChatPart } from "./types";

/** Which half of an agent a part belongs to: the Brain (the model) thinks, replies and asks for
 * tools; the Arms run them in the sandbox. */
export type Role = "brain-think" | "brain-reply" | "brain-ask" | "arms-act";

const requested = (t: ToolChatPart) => t.state === "composing" || t.state === "input";

/** True once the Arms have started a tool call; before that it is only the Brain's request. */
export const hasRun = (t: ToolChatPart) => !requested(t);

/** The icon family of a tool. */
export type ToolKind = "terminal" | "read" | "edit" | "find";

/** The icon family for a pi tool name; null draws the generic tool icon. */
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

/** What a part's avatar shows: `compose` is the Brain still writing a tool call, `request` the
 * finished call waiting to run, `asked` that call once the Arms took it, `unrun` a call the run
 * ended without running. */
export type Frame = "think" | "reply" | "compose" | "request" | "asked" | "act" | "done" | "failed" | "unrun";

/** The frame a part's avatar shows; null for parts with no avatar. */
export function frameOf(part: ChatPart, run: RunState): Frame | null {
    switch (part.type) {
        case "reasoning":
            return "think";
        case "text":
            return "reply";
        case "tool":
            switch (part.state) {
                case "composing":
                    return run === "running" ? "compose" : "unrun";
                case "input":
                    return run === "running" ? "request" : "unrun";
                case "running":
                    return "act";
                case "output":
                    return "done";
                case "error":
                    return "failed";
            }
        default:
            return null;
    }
}

/** True while the part is in progress: its avatar animates, then rests once the part finishes. */
export function isLive(part: ChatPart, run: RunState): boolean {
    if (run !== "running") return false;
    if (part.type === "tool") return requested(part) || part.state === "running";
    return (part.type === "text" || part.type === "reasoning") && part.streaming;
}

/** The frame of a tool call's Brain row: the request itself never fails, only its execution. */
export function askFrameOf(frame: Frame | null): Frame | null {
    return frame === "act" || frame === "done" || frame === "failed" ? "asked" : frame;
}
