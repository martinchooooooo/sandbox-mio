// Tool tiles: the things an agent's arms pick up, each good for one job.
import type { Core } from "./core";

export type ToolKind =
    | "terminal" | "read" | "edit" | "find" | "web" | "browser"
    | "todo" | "image" | "memory" | "email" | "calendar" | "ask";

export function createTools(core: Core) {
    const { el } = core;

    const page = (g: Element) => {
        el("path", { class: "ico", d: "M-11 -15 H4 L11 -8 V15 H-11 Z" }, g);
        el("path", { class: "ico line", d: "M4 -15 V-8 H11" }, g);
    };

    // Each icon fits a 32x30 box centred on the tile's origin.
    const ICONS: Record<ToolKind, (g: Element) => void> = {
        terminal(g) {
            el("rect", { class: "ico solid", x: -16, y: -13, width: 32, height: 26, rx: 3 }, g);
            el("text", { class: "term", x: -11, y: 4 }, g, ">_");
        },
        read(g) {
            page(g);
            el("path", { class: "ico line", d: "M-6 -2 H6 M-6 4 H6 M-6 10 H2" }, g);
        },
        edit(g) {
            page(g);
            el("path", { class: "ico line", d: "M-6 -2 H4 M-6 4 H0" }, g);
            el("path", { class: "pencil", d: "M0 14 L13 1" }, g);
        },
        find(g) {
            el("circle", { class: "ico", cx: -3, cy: -3, r: 9 }, g);
            el("path", { class: "ico line thick", d: "M4 4 L13 13" }, g);
        },
        web(g) {
            el("circle", { class: "ico", r: 14 }, g);
            el("ellipse", { class: "ico line", rx: 6, ry: 14 }, g);
            el("path", { class: "ico line", d: "M-14 0 H14" }, g);
        },
        browser(g) {
            el("rect", { class: "ico", x: -16, y: -13, width: 32, height: 26, rx: 3 }, g);
            el("path", { class: "ico line", d: "M-16 -5 H16" }, g);
            [-11, -6, -1].forEach((cx) => el("circle", { class: "pip", cx, cy: -9, r: 1.8 }, g));
        },
        todo(g) {
            [-9, 0, 9].forEach((y, i) => {
                el("rect", { class: i < 2 ? "ico warm" : "ico", x: -14, y: y - 3.5, width: 7, height: 7, rx: 1.5 }, g);
                el("path", { class: "ico line", d: `M-3 ${y} H14` }, g);
            });
        },
        image(g) {
            el("rect", { class: "ico", x: -15, y: -12, width: 30, height: 24, rx: 3 }, g);
            el("path", { class: "ico line", d: "M-15 10 L-5 -1 L3 7 L7 3 L15 10" }, g);
            el("circle", { class: "ico warm", cx: 7, cy: -5, r: 3 }, g);
        },
        memory(g) {
            el("path", { class: "ico warm", d: "M-9 -15 H9 V15 L0 8 L-9 15 Z" }, g);
        },
        email(g) {
            el("rect", { class: "ico", x: -16, y: -11, width: 32, height: 22, rx: 3 }, g);
            el("path", { class: "ico line", d: "M-16 -10 L0 3 L16 -10" }, g);
        },
        calendar(g) {
            el("rect", { class: "ico", x: -14, y: -12, width: 28, height: 26, rx: 3 }, g);
            el("path", { class: "ico warm", d: "M-14 -4 V-9 a3 3 0 0 1 3 -3 H11 a3 3 0 0 1 3 3 V-4 Z" }, g);
            for (let r = 0; r < 2; r++)
                for (let c = 0; c < 3; c++) el("circle", { class: "pip", cx: -7 + c * 7, cy: 3 + r * 6, r: 1.8 }, g);
        },
        ask(g) {
            el("path", { class: "ico", d: "M-15 -13 H15 V7 H-3 L-10 13 V7 H-15 Z" }, g);
            el("text", { class: "qmark", y: 2, "text-anchor": "middle" }, g, "?");
        },
    };

    /**
     * Draw a tool tile at (x, y) and return its outer group, which takes the 'hot', 'used' and
     * 'dim' classes. The entrance animation sits on an inner group because the outer one is
     * positioned by a transform attribute.
     */
    function tool(parent: Element, kind: ToolKind, label: string, x: number, y: number): SVGGElement {
        const g = el("g", { class: "tile", transform: `translate(${x} ${y})` }, parent);
        const body = el("g", { class: "rise" }, g);
        el("rect", { class: "ink frame", x: -28, y: -28, width: 56, height: 56, rx: 10 }, body);
        ICONS[kind](body);
        if (label) el("text", { class: "tlabel", y: 44, "text-anchor": "middle" }, body, label);
        return g;
    }

    return { tool };
}
