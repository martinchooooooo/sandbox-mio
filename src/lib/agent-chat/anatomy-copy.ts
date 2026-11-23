// Copy for learning mode: the explainer shown by each avatar's info icon and the avatar labels.
// TODO(i18n): every string in this file.
import type { Frame, Role } from "./anatomy";

interface RoleCopy {
    title: string;
    body: string;
}

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

/** The participant label of a part's avatar row, like a name in a group chat. */
export function avatarLabel(role: Role, frame: Frame | null, tool: string, live: boolean): string {
    switch (role) {
        case "brain-think":
            return live ? "Brain · thinking" : "Brain · thought";
        case "brain-reply":
            return "Brain · reply";
        case "brain-ask":
            if (frame === "compose") return "Brain · writing a request…";
            return frame === "request" ? `Brain · asking for ${tool}` : `Brain · asked for ${tool}`;
        case "arms-act":
            if (frame === "failed") return `Arms · ${tool} failed`;
            return frame === "act" ? `Arms · running ${tool}` : `Arms · ran ${tool}`;
    }
}
