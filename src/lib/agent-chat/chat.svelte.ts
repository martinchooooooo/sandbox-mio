import { Channel, invoke } from "@tauri-apps/api/core";
import { toast } from "svelte-sonner";
import { parseError } from "$lib/errors";
import { AGENTS, onboardingSeed } from "$lib/agents/catalogue";
import { PI_PINNED_VERSION } from "$lib/agents/pi";
import { provisionAgent } from "$lib/agents/provision";
import type { InferenceStatus } from "$lib/models/types";
import { applyPiRecord, emptyChatState, messagesFromHistory, resolveDialog, toPendingDialog } from "./pi-events";
import type {
    AttachEvent,
    AttachResult,
    ChatSession,
    ChatState,
    Cursor,
    HostStatus,
    PiEffect,
    PiRecord,
} from "./types";

/** Module-scoped chat store, one per sandbox, so a conversation survives tab and route changes.
 * Pi owns history: a fresh attach rebuilds it with `get_messages`; only a reconnect replays the
 * journal from the stored cursor (`docs/features/native-agent-ui/02_dev.md`, Lifecycle). */

/** `stopped` is the one detached state the view does not reattach from on its own: attaching
 * would start pi again right after the user stopped it. */
export type Connection = "idle" | "attaching" | "attached" | "disconnected" | "stopped" | "deployed" | "error";

/** One entry of `get_available_models`. */
export interface PiModel {
    id: string;
    name: string;
    provider: string;
    reasoning: boolean;
    contextWindow: number;
}

/** pi's thinking levels, used when `get_available_thinking_levels` is not answered. */
const DEFAULT_THINKING_LEVELS = ["off", "minimal", "low", "medium", "high"];

/** -1, 0 or 1 comparing dotted numeric versions; an unparsable part counts as 0. */
export function compareVersions(a: string, b: string): number {
    const pa = a.split(".").map((n) => Number.parseInt(n, 10) || 0);
    const pb = b.split(".").map((n) => Number.parseInt(n, 10) || 0);
    for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
        const d = (pa[i] ?? 0) - (pb[i] ?? 0);
        if (d !== 0) return Math.sign(d);
    }
    return 0;
}

/** How a message typed during a run reaches pi. */
export type DeliveryMode = "steer" | "follow_up";

const REQUEST_TIMEOUT_MS = 30_000;

type Pending = { resolve: (data: unknown) => void; reject: (e: Error) => void; timer: ReturnType<typeof setTimeout> };

export class ChatStore {
    readonly sandbox: string;
    workspacePath = $state("");
    connection = $state<Connection>("idle");
    error = $state<string | null>(null);
    writer = $state(true);
    host = $state<HostStatus | null>(null);
    chat = $state<ChatState>(emptyChatState());
    /** The `get_state` payload: model, thinking level, session file. */
    piState = $state<Record<string, unknown> | null>(null);
    deliveryMode = $state<DeliveryMode>("steer");
    /** Text an extension asked to place in the prompt input (`set_editor_text`). */
    editorText = $state<string | null>(null);
    models = $state<PiModel[]>([]);
    thinkingLevels = $state<string[]>(DEFAULT_THINKING_LEVELS);
    sessions = $state<ChatSession[]>([]);
    /** Set when a run failed and no inference server is running. */
    inferenceDown = $state(false);
    busy = $state<string | null>(null);

    #cursor: Cursor | null = null;
    #channel: Channel<AttachEvent> | null = null;
    #pending = new Map<string, Pending>();
    #seq = 0;
    #queued: AttachEvent[] = [];
    #frame: number | null = null;

    constructor(sandbox: string) {
        this.sandbox = sandbox;
    }

    /** True when the running pi is older than the pinned version the adapter is written against
     * (`docs/features/native-agent-ui/02_dev.md`, Resolved Decision 6). */
    get piOutdated(): boolean {
        const version = this.host?.piVersion ?? "";
        return version !== "" && compareVersions(version, PI_PINNED_VERSION) < 0;
    }

    /** Context-window use of the last assistant turn, or null before any usage is known. */
    get contextUse(): { used: number; window: number } | null {
        const model = this.piState?.model as { contextWindow?: unknown } | null | undefined;
        const window = Number(model?.contextWindow ?? 0);
        const usage = this.chat.usage;
        if (!usage || window <= 0) return null;
        return { used: usage.input + usage.cacheRead + usage.output, window };
    }

    async attach(): Promise<void> {
        if (this.connection === "attaching") return;
        this.connection = "attaching";
        this.error = null;
        const channel = new Channel<AttachEvent>();
        this.#channel = channel;
        // A replaced attach's channel can still deliver its final `mio_detached`; ignore it.
        channel.onmessage = (event) => {
            if (this.#channel === channel) this.#receive(event);
        };
        try {
            const result = await invoke<AttachResult>("agent_chat_attach", {
                name: this.sandbox,
                cursor: this.#cursor,
                onEvent: channel,
            });
            if (this.#channel !== channel) return;
            const fresh = this.#cursor?.generation !== result.attach.generation;
            this.#cursor = { generation: result.attach.generation, offset: result.attach.offset };
            this.writer = result.attach.writer;
            this.host = result.status;
            this.connection = "attached";
            if (fresh) await this.#rebuild(result.status);
        } catch (e) {
            if (this.#channel !== channel) return;
            this.#channel = null;
            const err = parseError(e);
            this.connection = err.kind === "AgentChatDeployed" ? "deployed" : "error";
            this.error = err.message;
        }
    }

    async detach(): Promise<void> {
        this.#channel = null;
        this.connection = "idle";
        this.#rejectAll("detached"); // TODO(i18n)
        try {
            await invoke("agent_chat_detach", { name: this.sandbox });
        } catch (e) {
            console.error(parseError(e).message);
        }
    }

    /** Sends one command and resolves with its response's `data`. */
    async request(command: Record<string, unknown>): Promise<unknown> {
        const id = `mio-${++this.#seq}`;
        const line = JSON.stringify({ ...command, id });
        const response = new Promise<unknown>((resolve, reject) => {
            const timer = setTimeout(() => {
                this.#pending.delete(id);
                reject(new Error(`${String(command.type)} timed out`)); // TODO(i18n)
            }, REQUEST_TIMEOUT_MS);
            this.#pending.set(id, { resolve, reject, timer });
        });
        try {
            await invoke("agent_chat_send", { name: this.sandbox, line });
        } catch (e) {
            const pending = this.#pending.get(id);
            if (pending) clearTimeout(pending.timer);
            this.#pending.delete(id);
            throw new Error(parseError(e).message);
        }
        return response;
    }

    /** Sends a message: a prompt when idle, otherwise a steer or follow-up, because pi rejects a
     * plain prompt during a run (Common Gotchas). */
    async sendMessage(text: string): Promise<void> {
        const type = this.chat.run === "idle" ? "prompt" : this.deliveryMode;
        await this.request({ type, message: text });
    }

    async abort(): Promise<void> {
        await this.request({ type: "abort" });
    }

    /** Answers a pending dialog; `answer` is the response's payload (`value`, `confirmed` or `cancelled`). */
    async answerDialog(id: string, answer: Record<string, unknown>): Promise<void> {
        // Not a request: pi sends no response to a dialog answer.
        const line = JSON.stringify({ type: "extension_ui_response", id, ...answer });
        await invoke("agent_chat_send", { name: this.sandbox, line });
        resolveDialog(this.chat, id);
    }

    /** Reloads model, thinking level and history from pi. */
    async refresh(): Promise<void> {
        const state = (await this.request({ type: "get_state" })) as Record<string, unknown>;
        this.piState = state;
        const messages = (await this.request({ type: "get_messages" })) as { messages?: unknown[] };
        this.chat.messages = messagesFromHistory(this.chat, messages?.messages ?? []);
        this.chat.streaming = null;
        if (state?.isStreaming === true) this.chat.run = "running";
    }

    /** Model list and thinking levels for the selectors. */
    async loadModels(): Promise<void> {
        try {
            const data = (await this.request({ type: "get_available_models" })) as { models?: PiModel[] };
            this.models = data?.models ?? [];
        } catch (e) {
            console.error(e);
        }
        try {
            const data = (await this.request({ type: "get_available_thinking_levels" })) as { levels?: string[] };
            this.thinkingLevels = data?.levels?.length ? data.levels : DEFAULT_THINKING_LEVELS;
        } catch {
            this.thinkingLevels = DEFAULT_THINKING_LEVELS;
        }
    }

    async setModel(model: PiModel): Promise<void> {
        await this.#act("model", async () => {
            await this.request({ type: "set_model", provider: model.provider, modelId: model.id });
            this.piState = (await this.request({ type: "get_state" })) as Record<string, unknown>;
        });
    }

    async setThinkingLevel(level: string): Promise<void> {
        await this.#act("thinking", async () => {
            await this.request({ type: "set_thinking_level", level });
            this.piState = (await this.request({ type: "get_state" })) as Record<string, unknown>;
        });
    }

    async loadSessions(): Promise<void> {
        try {
            this.sessions = await invoke<ChatSession[]>("agent_chat_sessions", { workspacePath: this.workspacePath });
        } catch (e) {
            console.error(parseError(e).message);
        }
    }

    async newSession(): Promise<void> {
        await this.#act("session", async () => {
            await this.request({ type: "new_session" });
            await this.refresh();
        });
    }

    /** Switches by the sandbox path; the host path of a session file does not resolve inside the sandbox. */
    async switchSession(session: ChatSession): Promise<void> {
        await this.#act("session", async () => {
            await this.request({ type: "switch_session", sessionPath: session.sandboxPath });
            await this.refresh();
        });
    }

    /** Ends the other client's writer stream, then reattaches from the cursor as the writer. */
    async takeControl(): Promise<void> {
        await this.#act("takeover", async () => {
            await invoke("agent_chat_takeover", { name: this.sandbox });
            this.connection = "idle";
            await this.attach();
        });
    }

    async stopAgent(): Promise<void> {
        await this.#act("stop", async () => {
            this.#channel = null;
            await invoke("agent_chat_stop", { name: this.sandbox });
            this.#rejectAll("stopped"); // TODO(i18n)
            this.chat.run = "idle";
            this.connection = "stopped";
        });
    }

    /** Reinstalls the pinned pi, then restarts the supervisor so the new binary runs. */
    async updatePi(): Promise<void> {
        const pi = AGENTS.find((a) => a.id === "pi");
        if (!pi) return;
        await this.#act("update", async () => {
            await invoke("install_agent", {
                name: this.sandbox,
                packages: pi.misePackages,
                postinstallNpm: null,
                credentialService: pi.credentialService ?? null,
                onboarding: onboardingSeed(pi),
                onProgress: new Channel(),
            });
            const status = await invoke<InferenceStatus>("inference_status").catch(() => null);
            await provisionAgent(pi, this.workspacePath, status?.running ? status.modelId : null);
            this.#channel = null;
            await invoke("agent_chat_stop", { name: this.sandbox });
            this.#cursor = null;
            this.connection = "idle";
            await this.attach();
        });
    }

    async #act(name: string, body: () => Promise<void>): Promise<void> {
        this.busy = name;
        try {
            await body();
        } catch (e) {
            toast.error(e instanceof Error ? e.message : parseError(e).message);
        } finally {
            this.busy = null;
        }
    }

    /** A failed run with no server running means pi cannot reach a model at all. */
    async #checkInference(): Promise<void> {
        if (!this.chat.lastRunErrored) {
            this.inferenceDown = false;
            return;
        }
        try {
            const status = await invoke<InferenceStatus>("inference_status");
            this.inferenceDown = !status.running;
        } catch {
            this.inferenceDown = false;
        }
    }

    async #rebuild(status: HostStatus): Promise<void> {
        this.chat = emptyChatState();
        this.chat.dialogs = status.pendingDialogs.flatMap((r) => toPendingDialog(r) ?? []);
        try {
            await this.refresh();
        } catch (e) {
            this.error = e instanceof Error ? e.message : String(e);
        }
        void this.loadModels();
    }

    #receive(event: AttachEvent): void {
        const record = event.record;
        if (record.type === "mio_detached") {
            this.#channel = null;
            this.connection = "disconnected";
            this.#rejectAll("disconnected"); // TODO(i18n)
            return;
        }
        if (event.end !== null && this.#cursor) this.#cursor.offset = event.end;
        // Responses resolve immediately; everything else re-renders at most once per frame
        // (`docs/features/native-agent-ui/02_dev.md`, Resolved Decision 10).
        if (record.type === "response") {
            this.#resolve(record);
            return;
        }
        this.#queued.push(event);
        this.#frame ??= requestAnimationFrame(() => this.#flush());
    }

    #flush(): void {
        this.#frame = null;
        const events = this.#queued;
        this.#queued = [];
        const effects: PiEffect[] = [];
        for (const event of events) effects.push(...applyPiRecord(this.chat, event.record));
        for (const effect of effects) this.#perform(effect);
    }

    #perform(effect: PiEffect): void {
        switch (effect.kind) {
            case "notify":
                if (effect.level === "error") toast.error(effect.message);
                else if (effect.level === "warning") toast.warning(effect.message);
                else toast.info(effect.message);
                return;
            case "setEditorText":
                this.editorText = effect.text;
                return;
            case "settled":
                void this.#checkInference();
                return;
        }
    }

    #resolve(record: PiRecord): void {
        const id = typeof record.id === "string" ? record.id : "";
        const pending = this.#pending.get(id);
        if (!pending) return;
        clearTimeout(pending.timer);
        this.#pending.delete(id);
        if (record.success === false) pending.reject(new Error(String(record.error ?? "command failed"))); // TODO(i18n)
        else pending.resolve(record.data);
    }

    #rejectAll(reason: string): void {
        for (const pending of this.#pending.values()) {
            clearTimeout(pending.timer);
            pending.reject(new Error(reason));
        }
        this.#pending.clear();
    }
}

const stores = new Map<string, ChatStore>();

/** The chat store for `sandbox`, created on first use; does not attach. */
export function ensureChat(sandbox: string, workspacePath: string): ChatStore {
    let store = stores.get(sandbox);
    if (!store) {
        store = new ChatStore(sandbox);
        stores.set(sandbox, store);
    }
    store.workspacePath = workspacePath;
    return store;
}
