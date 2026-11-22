// The three non-computer characters: You, the Chat window, and the Brain (the model).
// TODO(i18n): every label and placeholder drawn in this file.
import type { Board } from "./board";
import type { Core } from "./core";

/** A brand look for a brain or a pair of arms: its fill, and optional multicoloured folds. */
export interface Look {
    fill: string;
    folds?: string[];
}

const FOLDS = [
    "M0 -58 C-12 -30 12 -12 0 8 C-10 28 8 44 0 58",
    "M-42 -22 q14 -12 26 4",
    "M-46 14 q16 -10 30 6",
    "M-28 -44 q12 2 12 16",
    "M-30 40 q4 -14 18 -12",
    "M42 -22 q-14 -12 -26 4",
    "M46 14 q-16 -10 -30 6",
    "M28 -44 q-12 2 -12 16",
    "M30 40 q-4 -14 -18 -12",
];

/** True for brand colours dark enough to need white marks on top (brain folds, badge letters). */
export function isDark(hex: string): boolean {
    const n = parseInt(hex.slice(1), 16);
    return 0.299 * (n >> 16) + 0.587 * ((n >> 8) & 255) + 0.114 * (n & 255) < 120;
}

export function createCharacters(core: Core, board: Board) {
    const { el } = core;
    const { actor } = board;

    const you = actor("you", "You", 64);
    el("path", { class: "ink fill-accent", d: "M-34 42 Q-34 0 0 0 Q34 0 34 42 Z" }, you);
    el("circle", { class: "ink fill-accent", cx: 0, cy: -26, r: 20 }, you);

    const chat = actor("chat", "Chat", 118);
    el("rect", { class: "ink", x: -110, y: -95, width: 220, height: 190, rx: 12 }, chat);
    el("path", { class: "ink fill-ink", d: "M-110 -71 V-83 a12 12 0 0 1 12 -12 H98 a12 12 0 0 1 12 12 V-71 Z" }, chat);
    for (let i = 0; i < 3; i++) el("circle", { cx: -94 + i * 11, cy: -83, r: 3, fill: "#fff" }, chat);
    const bubbles = el("g", {}, chat);
    el("rect", { class: "ink thin", x: -100, y: 67, width: 200, height: 20, rx: 10 }, chat);
    el("text", { class: "tiny faint", x: -90, y: 80 }, chat, "Type a message…");

    let chatY = -62;
    function chatClear() {
        bubbles.replaceChildren();
        chatY = -62;
    }
    core.resets.push(chatClear);

    /** Add a chat bubble. who: 'me' (right, orange) or 'ai' (left, peach); lines are pre-wrapped. */
    function chatSay(who: "me" | "ai", lines: string[]) {
        const g = el("g", { class: "bubble " + who }, bubbles);
        const rect = el("rect", { rx: 8 }, g);
        const texts = lines.map((line, i) => el("text", { y: chatY + 15 + i * 13 }, g, line));
        const w = Math.max(...texts.map((t) => t.getComputedTextLength())) + 16;
        const h = lines.length * 13 + 9;
        const x = who === "me" ? 100 - w : -100;
        rect.setAttribute("x", String(x));
        rect.setAttribute("y", String(chatY));
        rect.setAttribute("width", String(w));
        rect.setAttribute("height", String(h));
        texts.forEach((t) => t.setAttribute("x", String(x + 8)));
        chatY += h + 6;
    }

    /** Recolour a brain inside `parent` with a maker's look; null restores the default. */
    function paintBrain(parent: Element, look: Look | null) {
        const body = parent.querySelector<SVGCircleElement>("circle");
        if (body) body.style.fill = look ? look.fill : "";
        const folds = look?.folds || (look && isDark(look.fill) ? ["#fff"] : null);
        parent.querySelectorAll<SVGPathElement>(".fold").forEach((f, i) => {
            f.style.stroke = folds ? folds[i % folds.length] : "";
            // Multicoloured folds are the whole logo, so they are drawn bolder.
            f.style.strokeWidth = look?.folds ? "4.5" : "";
        });
    }

    /** Draw a brain of radius 58 into `parent`, optionally in a maker's colours. */
    function drawBrain(parent: Element, look?: Look | null) {
        el("circle", { class: "ink fill-muted", r: 58 }, parent);
        FOLDS.forEach((d) => el("path", { class: "fold", d }, parent));
        if (look) paintBrain(parent, look);
    }

    const brain = actor("brain", "Model · the brain", 84);
    drawBrain(brain);
    core.resets.push(() => paintBrain(brain, null));

    return { chatClear, chatSay, paintBrain, drawBrain, isDark };
}
