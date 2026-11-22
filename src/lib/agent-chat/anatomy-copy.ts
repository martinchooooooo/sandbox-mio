// Explainer copy for the learning panel, in the onboarding book's vocabulary.
// TODO(i18n): every string in this file.
import type { ToolKind } from "$lib/story-stage";
import type { Beat, BeatKind, Role } from "./anatomy";

interface RoleCopy {
    title: string;
    body: string;
}

export const INTRO: RoleCopy = {
    title: "Every agent is a Brain with Arms",
    body: "The Brain is the AI model: it only reads and writes text. The Arms are the tools it can use to actually do things in your sandbox. Send a message and watch them work together. Click a badge in the chat to learn what that part is.",
};

export const ROLE_COPY: Record<Role, RoleCopy> = {
    "brain-think": {
        title: "The Brain is thinking",
        body: "Before it answers, the Brain works through the problem in private notes. Nothing changes in your sandbox while it thinks. You can open the notes to read them.",
    },
    "brain-reply": {
        title: "The Brain is replying",
        body: "This is the Brain's answer to you, written one word at a time. The Brain only produces text, so when it needs something done, it asks for a tool instead.",
    },
    "brain-ask": {
        title: "The Brain asks for a tool",
        body: "A model cannot touch files or run programs. So it writes a request naming a tool and the details it needs, and the Arms carry that request out.",
    },
    "arms-act": {
        title: "The Arms use a tool",
        body: "The Arms do the work inside your sandbox, a pretend computer kept apart from your real one. When they finish, the result goes back to the Brain, which decides what to do next.",
    },
};

/** The role whose explainer fits what the agent is doing, or null while idle. */
export function roleForBeat(kind: BeatKind): Role | null {
    switch (kind) {
        case "thinking":
        case "retrying":
            return "brain-think";
        case "asking":
            return "brain-ask";
        case "acting":
        case "returning":
            return "arms-act";
        case "replying":
            return "brain-reply";
        default:
            return null;
    }
}

const ACTING_LINES: Partial<Record<ToolKind, string>> = {
    terminal: "The Arms are running a command in your sandbox",
    read: "The Arms are reading a file in your sandbox",
    edit: "The Arms are changing a file in your sandbox",
    find: "The Arms are looking for files in your sandbox",
};

/** The tool word on an Arms badge. */
export const ARMS_WORDS: Partial<Record<ToolKind, string>> = {
    terminal: "terminal",
    read: "reading",
    edit: "editing",
    find: "searching",
};

/** The one-line sentence under the stage. */
export function beatLine(beat: Beat): string {
    switch (beat.kind) {
        case "idle":
            return "Waiting for your next message";
        case "thinking":
            return "The Brain is thinking…";
        case "asking":
            return "The Brain is asking for a tool";
        case "acting":
            return beat.toolKinds.length === 1 && beat.toolKinds[0]
                ? ACTING_LINES[beat.toolKinds[0]] ?? "The Arms are using a tool in your sandbox"
                : "The Arms are using tools in your sandbox";
        case "returning":
            return "The result goes back to the Brain";
        case "replying":
            return "The Brain is writing its reply";
        case "retrying":
            return "The connection to the Brain dropped; pi is trying again";
    }
}
