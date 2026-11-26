import { Channel, invoke } from "@tauri-apps/api/core";
import type { AttachEvent, AttachResult, ChatSession, Cursor } from "$lib/agent-chat/types";

/** Streams journal records to `onEvent` until detached; resumes after `cursor` when its generation still matches. */
export function agentChatAttach(
    name: string,
    cursor: Cursor | null,
    onEvent: Channel<AttachEvent>,
): Promise<AttachResult> {
    return invoke<AttachResult>("agent_chat_attach", { name, cursor, onEvent });
}

export function agentChatDetach(name: string): Promise<void> {
    return invoke("agent_chat_detach", { name });
}

/** `line` is one JSON-encoded pi RPC command. */
export function agentChatSend(name: string, line: string): Promise<void> {
    return invoke("agent_chat_send", { name, line });
}

export function agentChatTakeover(name: string): Promise<void> {
    return invoke("agent_chat_takeover", { name });
}

export function agentChatStop(name: string): Promise<void> {
    return invoke("agent_chat_stop", { name });
}

export function agentChatSessions(workspacePath: string): Promise<ChatSession[]> {
    return invoke<ChatSession[]>("agent_chat_sessions", { workspacePath });
}
