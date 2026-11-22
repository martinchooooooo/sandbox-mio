import { CANCEL, type Layout, type StoryStage, type ToolKind } from "$lib/story-stage";
import { sameBeat, type Beat, type BeatKind } from "./anatomy";

/** The live stage's portrait board, in stage units. */
export const BOARD = { width: 400, height: 440 };

export const LAYOUT: Layout = {
    brain: [200, 95, 0.85],
    box: [200, 345, 0.85],
};

/** Floors on how long a beat stays drawn: tool calls run in 7 to 90 ms, far too fast to see
 * (docs/features/chat-agent-anatomy/02_dev.md, Resolved Decision 6). */
const MIN_MS: Partial<Record<BeatKind, number>> = { asking: 600, acting: 1200, returning: 600 };

/** More waiting beats than this and the driver drops the stale ones and plays the newest. */
const MAX_WAITING = 3;

export interface Driver {
    /** Queues a beat; each plays for at least its minimum duration, in order. */
    push(beat: Beat): void;
    /** Draws a beat's end state at once and drops everything queued; for history and reduced motion. */
    settle(beat: Beat): void;
    dispose(): void;
}

/**
 * Plays chat beats on a stage laid out with `LAYOUT`. It owns only its queue; a beat is always
 * derived from chat state, so a reconnect rebuilds the same picture.
 */
export function createDriver(stage: StoryStage, onDraw: (kind: BeatKind) => void): Driver {
    const { layers, at, sleep, send, jolt, think, typing, reach, retract, carry, carryCount, keys, tool } = stage;

    let queue: Beat[] = [];
    let last: Beat | null = null;
    let current: Beat | null = null;
    let generation = 0;
    let playing = false;
    let disposed = false;

    const kindsOf = (beat: Beat): ToolKind[] => beat.toolKinds.map((k) => k ?? "ask");

    function clear() {
        think(false);
        typing(false);
        carry([]);
        carryCount(0);
        layers.fx.replaceChildren();
    }

    async function drawActing(beat: Beat, ms?: number) {
        const kinds = kindsOf(beat);
        carry(kinds);
        carryCount(kinds.length - 2);
        await reach(keys("box"), ms);
        typing(true);
    }

    async function play(beat: Beat): Promise<void> {
        const started = performance.now();
        current = beat;
        onDraw(beat.kind);
        clear();
        switch (beat.kind) {
            case "thinking":
                think(true);
                await retract(300);
                break;
            case "asking":
                kindsOf(beat)
                    .slice(0, 2)
                    .forEach((kind, i) => {
                        const [x, y] = at("brain", 105 + i * 70, -10);
                        tool(layers.fx, kind, "", x, y);
                    });
                await retract(300);
                break;
            case "acting":
                await drawActing(beat);
                break;
            case "returning": {
                const [from, to] = [at("box", 0, -20), at("brain", 0, 0)];
                void send(from, to, "result", "cmd", 600).catch(() => {});
                await retract(500);
                break;
            }
            case "replying": {
                await retract(300);
                const from = at("brain", 50, -30);
                void send(from, [BOARD.width - 40, from[1]], "Reply", "reply", 700).catch(() => {}); // TODO(i18n)
                break;
            }
            case "retrying":
                await retract(300);
                jolt("brain", "shake");
                break;
            case "idle":
                await retract(300);
                break;
        }
        const wait = (MIN_MS[beat.kind] ?? 0) - (performance.now() - started);
        if (wait > 0) await sleep(wait);
    }

    async function run(gen: number) {
        try {
            while (gen === generation && queue.length > 0 && !disposed) await play(queue.shift()!);
        } catch (e) {
            if (e !== CANCEL) console.error(e);
        } finally {
            if (gen === generation) playing = false;
        }
    }

    function start() {
        playing = true;
        void run(++generation);
    }

    function skipTo(beat: Beat) {
        stage.cancelAll();
        queue = [beat];
        start();
    }

    return {
        push(beat) {
            if (disposed || (last && sameBeat(last, beat))) return;
            last = beat;
            if (beat.kind === "acting") {
                // Parallel calls start milliseconds apart: grow the tiles instead of replaying the beat.
                if (queue.at(-1)?.kind === "acting") {
                    queue[queue.length - 1] = beat;
                    return;
                }
                if (queue.length === 0 && current?.kind === "acting") {
                    current = beat;
                    const kinds = kindsOf(beat);
                    carry(kinds);
                    carryCount(kinds.length - 2);
                    return;
                }
            }
            queue.push(beat);
            if (queue.length > MAX_WAITING) skipTo(beat);
            else if (!playing) start();
        },
        settle(beat) {
            if (disposed) return;
            last = beat;
            current = beat;
            stage.cancelAll();
            queue = [];
            generation++;
            playing = false;
            onDraw(beat.kind);
            clear();
            void retract(0);
            if (beat.kind === "thinking") think(true);
            if (beat.kind === "acting") void drawActing(beat, 0);
        },
        dispose() {
            disposed = true;
            stage.cancelAll();
        },
    };
}
