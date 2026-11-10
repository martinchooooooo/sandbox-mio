// The SVG board: paint layers, the actor registry, placement, and the caption line.
import type { Core } from "./core";

/** `[x, y, scale = 1, opacity = 1]` in stage coordinates (the viewBox is 1000 x 440). */
export type Placement = [number, number, number?, number?];

/** Actor name to placement; an actor missing from the layout is hidden. */
export type Layout = Record<string, Placement | undefined>;

interface Actor {
    g: SVGGElement;
    inner: SVGGElement;
    label: SVGTextElement | null;
    labelText: string;
}

export function createBoard(svg: SVGSVGElement, core: Core, onCaption: (text: string) => void) {
    const { el } = core;

    // Paint order, back to front.
    const layers = {
        fxBack: el("g", {}, svg),
        arms: el("g", { class: "arms" }, svg),
        actors: el("g", {}, svg),
        fx: el("g", {}, svg),
    };

    const actors: Record<string, Actor> = {};
    /** Last placement per actor, or null while hidden. */
    const pos: Record<string, [number, number, number] | null> = {};

    /** Register an actor; draw its shape into the returned `inner` group, in its own coordinates. */
    function actor(name: string, label: string, labelY: number): SVGGElement {
        const g = el("g", { class: "actor" }, layers.actors);
        g.style.transform = "translate(500px,220px) scale(0.6)";
        g.style.opacity = "0";
        const inner = el("g", { class: "inner" }, g);
        const labelEl = label
            ? el("text", { class: "label", y: labelY, "text-anchor": "middle" }, g, label)
            : null;
        actors[name] = { g, inner, label: labelEl, labelText: label };
        return inner;
    }

    /**
     * Replay a one-off animation class on `node` (default: the small entrance).
     * `node` must not carry a transform attribute: the animation would override it.
     */
    function pop(node: Element, cls = "rise") {
        node.classList.remove(cls);
        void node.getBoundingClientRect();
        node.classList.add(cls);
    }

    /** Rename an actor for the current page only; `resetStage` puts the original label back. */
    function relabel(name: string, text: string) {
        const { label } = actors[name];
        if (!label) return;
        label.textContent = text;
        pop(label);
    }

    /** Place an actor, or pass a missing placement to hide it. CSS transitions animate the move. */
    function place(name: string, p: Placement | undefined | null) {
        const g = actors[name].g;
        if (!p) {
            g.style.opacity = "0";
            g.style.pointerEvents = "none";
            pos[name] = null;
            return;
        }
        const [x, y, s = 1, o = 1] = p;
        g.style.transform = `translate(${x}px,${y}px) scale(${s})`;
        g.style.opacity = String(o);
        g.style.pointerEvents = "auto";
        pos[name] = [x, y, s];
    }

    /** A point given in an actor's own coordinates, converted to stage coordinates. */
    function at(name: string, dx: number, dy: number): [number, number] {
        const p = pos[name];
        if (!p) throw new Error(`actor "${name}" is not in this page's layout`);
        const [x, y, s] = p;
        return [x + dx * s, y + dy * s];
    }

    /** Show one short caption line under the stage. */
    const cap = (text: string) => onCaption(text);

    /** Wipe the stage and lay actors out for a page; other modules hook in through `resets`. */
    function resetStage(layout: Layout) {
        layers.fx.replaceChildren();
        layers.fxBack.replaceChildren();
        core.resets.forEach((fn) => fn());
        for (const name in actors) {
            const a = actors[name];
            a.inner.setAttribute("class", "inner");
            if (a.label) a.label.textContent = a.labelText;
            place(name, layout[name]);
        }
        onCaption("");
    }

    return { svg, layers, actors, pos, actor, pop, relabel, place, at, cap, resetStage };
}

export type Board = ReturnType<typeof createBoard>;
