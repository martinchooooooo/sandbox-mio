// The computers: `pc` (the user's real one), `host` (the same computer drawn big enough to hold
// the sandbox on its screen) and `box` (the sandbox). Screen contents live in screens.ts.
// TODO(i18n): every label drawn in this file.
import type { Board, Placement } from "./board";
import type { Core } from "./core";

export interface Computer {
    name: string;
    inner: SVGGElement;
    screen: SVGGElement;
    badge: SVGGElement;
    /** Next free terminal line. */
    line: number;
    /** Screen half-width and vertical extent, in the computer's own coordinates. */
    w: number;
    top: number;
    bottom: number;
    /** Where the hands rest: the left end of the keyboard. */
    keys: [number, number];
    badgeX: number;
    badgeY: number;
}

export function createComputers(core: Core, board: Board) {
    const { el } = core;
    const { actor, actors } = board;

    const computers: Record<string, Computer> = {};

    /** A laptop whose screen spans x -w..w, y top..bottom in its own coordinates. */
    function makeComputer(name: string, label: string, w = 90, top = -70, bottom = 50): Computer {
        const inner = actor(name, label, bottom + 44);
        el("rect", { class: "ink", x: -w - 8, y: top - 8, width: 2 * w + 16, height: bottom - top + 16, rx: 10 }, inner);
        const screen = el("g", {}, inner);
        el("path", { class: "ink fill-ink", d: `M${-w - 26} ${bottom + 8} H${w + 26} L${w + 14} ${bottom + 22} H${-w - 14} Z` }, inner);
        const badge = el("g", { class: "badge" }, inner);
        const c: Computer = {
            name, inner, screen, badge, line: 0, w, top, bottom,
            keys: [-w - 20, bottom + 10], badgeX: 0, badgeY: top - 42,
        };
        computers[name] = c;
        return c;
    }

    const pc = makeComputer("pc", "Your computer");
    // Registered before `box`, so the sandbox paints on top of its screen.
    const host = makeComputer("host", "Your computer", 220, -130, 100);
    host.badgeX = 122; // over the "My files" window `scrDesktop` draws
    const box = makeComputer("box", "");

    // The dashed boundary sits behind the laptop, so it goes first in the group.
    box.inner.insertBefore(
        el("rect", { class: "dash", x: -140, y: -112, width: 280, height: 206, rx: 16 }),
        box.inner.firstChild,
    );
    el("text", { class: "label", x: -138, y: -122 }, actors.box.g, "Sandbox · a pretend computer");

    // The sandbox's spot on the host's screen: the left half, clear of the "My files" window.
    const SLOT = [-95, -2, 0.8];

    /** The `box` placement that puts the sandbox on the host's screen, for a host placed at [x, y, s]. */
    const sandboxIn = ([x, y, s = 1]: Placement): Placement => [x + SLOT[0] * s, y + SLOT[1] * s, SLOT[2] * s];

    /** A pill above the computer; pass '' to remove it. */
    function setBadge(c: Computer, text: string) {
        c.badge.replaceChildren();
        if (!text) return;
        const rect = el("rect", { y: c.badgeY, height: 24, rx: 12 }, c.badge);
        const t = el("text", { x: c.badgeX, y: c.badgeY + 17, "text-anchor": "middle" }, c.badge, text);
        const w = t.getComputedTextLength() + 22;
        rect.setAttribute("x", String(c.badgeX - w / 2));
        rect.setAttribute("width", String(w));
    }

    core.resets.push(() => {
        for (const name in computers) setBadge(computers[name], "");
    });

    return { computers, pc, host, box, sandboxIn, setBadge };
}

export type Computers = ReturnType<typeof createComputers>;
