// One-shot effects: packets flying between actors, the brain thinking, shake and wobble.
import type { Board } from "./board";
import type { Core } from "./core";

type Point = [number, number];

export function createEffects(core: Core, board: Board) {
    const { el, tween } = core;
    const { layers, actors } = board;

    /** Fly a labelled pill from `a` to `b` (stage coordinates). cls: '' | 'reply' | 'cmd' | 'shared'. */
    async function send(a: Point, b: Point, label: string, cls = "", ms = 950) {
        const g = el("g", { class: "packet " + cls }, layers.fx);
        const rect = el("rect", { y: -11, height: 22, rx: 11 }, g);
        const t = el("text", { y: 4, "text-anchor": "middle" }, g, label);
        const w = t.getComputedTextLength() + 20;
        rect.setAttribute("x", String(-w / 2));
        rect.setAttribute("width", String(w));
        await tween(ms, (k) => {
            const x = a[0] + (b[0] - a[0]) * k;
            const y = a[1] + (b[1] - a[1]) * k - Math.sin(Math.PI * k) * 26;
            g.setAttribute("transform", `translate(${x} ${y})`);
        });
        g.remove();
    }

    /** A marching dashed line in the shared-folder colour from `a` to `b`: a folder and its copy. */
    const link = (a: Point, b: Point) =>
        el("path", { class: "link", d: `M${a[0]} ${a[1]} L${b[0]} ${b[1]}` }, layers.fx);

    /** Pulse the brain while it is "thinking". */
    const think = (on: boolean) => actors.brain.inner.classList.toggle("thinking", on);

    /** Play a one-off CSS animation class ('wobble' | 'shake') on an actor. */
    function jolt(name: string, cls: "wobble" | "shake") {
        const inner = actors[name].inner;
        inner.classList.remove(cls);
        void inner.getBoundingClientRect();
        inner.classList.add(cls);
    }

    return { send, link, think, jolt };
}
