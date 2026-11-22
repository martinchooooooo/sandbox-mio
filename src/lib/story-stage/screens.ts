// What appears on a computer's screen: a settings menu, a terminal, folders, a message.
// Each helper replaces the whole screen of the computer `c` it is given (see computers.ts).
// TODO(i18n): the settings menu names and window titles drawn in this file.
import type { Board } from "./board";
import type { Computer } from "./computers";
import type { Core } from "./core";

/** A folder or document icon; keeps its centre for things drawn on it (`lockFile`). */
export type FileIcon = SVGGElement & { x: number; y: number };
/** A settings-menu button, tagged with its vertical centre for the fake cursor. */
export type ScreenButton = SVGGElement & { cy: number };

const CURSOR = "M0 0 V14 L4 10.5 L7 16 L9.5 15 L6.5 9.5 H11 Z";

export function createScreens(core: Core, board: Board) {
    const { el, sleep, tween, clicked } = core;
    const { pop } = board;

    /** Blank screen with an optional title bar; returns the bar. */
    function scr(c: Computer, title?: string, dark?: boolean): SVGRectElement | null {
        c.screen.replaceChildren();
        c.line = 0;
        el("rect", { class: dark ? "scr dark" : "scr", x: -90, y: -70, width: 180, height: 120, rx: 4 }, c.screen);
        if (!title) return null;
        const bar = el("rect", { class: "scrbar", x: -90, y: -70, width: 180, height: 16, rx: 4 }, c.screen);
        el("text", { class: "tiny", x: -84, y: -59 }, c.screen, title);
        return bar;
    }

    function scrMsg(c: Computer, title: string, text: string) {
        scr(c, title);
        el("text", { class: "msg", x: 0, y: 0, "text-anchor": "middle" }, c.screen, text);
    }

    // ----- point-and-click -----

    /** The three settings buttons, in click order. */
    function scrSettings(c: Computer): ScreenButton[] {
        scr(c, "Your laptop");
        return ["Settings", "Accounts", "Change password"].map((name, i) => {
            const b = el("g", { class: "sbtn" }, c.screen);
            el("rect", { x: -74, y: -46 + i * 30, width: 148, height: 24, rx: 6 }, b);
            el("text", { x: -64, y: -30 + i * 30 }, b, name);
            return Object.assign(b, { cy: -34 + i * 30 });
        });
    }

    /** A mouse pointer on `c`'s screen, for beats where the story clicks for the reader. */
    function cursor(c: Computer): SVGPathElement {
        return el("path", { class: "cursor", d: CURSOR }, c.screen);
    }

    /**
     * Make each button glow in turn and wait for it. With `auto` a cursor clicks for the reader,
     * otherwise the reader must click.
     */
    async function clickThrough(c: Computer, btns: ScreenButton[], auto: boolean) {
        const pointer = auto ? cursor(c) : null;
        let from = [70, 40];
        for (const b of btns) {
            b.classList.add("hot");
            if (pointer) {
                const start = from;
                const to = [30, b.cy];
                await tween(600, (t) =>
                    pointer.setAttribute(
                        "transform",
                        `translate(${start[0] + (to[0] - start[0]) * t} ${start[1] + (to[1] - start[1]) * t})`,
                    ),
                );
                from = to;
                await sleep(300);
            } else {
                await clicked(b);
            }
            b.classList.remove("hot");
            b.classList.add("done");
            el("text", { x: 58, y: b.cy + 4 }, b, "✓");
            await sleep(350);
        }
    }

    // ----- terminal -----

    /** A terminal, optionally titled; returns the title bar so a page can colour it. */
    function scrTerm(c: Computer, title?: string): SVGRectElement | null {
        const bar = scr(c, title, true);
        if (bar) c.line = 1; // the bar covers the first line
        return bar;
    }

    function termLine(c: Computer, cls?: string) {
        return el("text", { class: "term " + (cls || ""), x: -82, y: -52 + c.line++ * 15 }, c.screen);
    }

    async function termType(c: Computer, text: string) {
        const line = termLine(c);
        line.textContent = "$ ";
        for (const ch of text) {
            line.textContent += ch;
            await sleep(70);
        }
        await sleep(350);
    }

    /** cls 'ok' prints green. */
    function termPrint(c: Computer, text: string, cls?: string) {
        termLine(c, cls).textContent = text;
    }

    // ----- folders -----

    function folder(parent: Element, name: string, x: number, y: number, cls?: string | false): FileIcon {
        const f = el("g", { class: cls ? "file " + cls : "file" }, parent);
        el("path", { d: `M${x - 18} ${y - 10} h12 l4 4 h20 v22 h-36 Z` }, f);
        el("text", { x, y: y + 27, "text-anchor": "middle" }, f, name);
        return Object.assign(f, { x, y });
    }

    /** Up to four folders in a 2x2 grid; returns them so `wreck` can delete some. */
    function scrFiles(c: Computer, title: string, names: string[], fresh?: boolean): FileIcon[] {
        scr(c, title);
        return names.map((name, i) => folder(c.screen, name, i % 2 ? 42 : -42, i < 2 ? -30 : 16, fresh && "fresh"));
    }

    /** Centre of slot `i` in the desktop's "My files" window: two columns, three rows. */
    const deskSlot = (i: number): [number, number] => [i % 2 ? 163 : 82, -62 + Math.floor(i / 2) * 52];

    /**
     * The big computer's desktop (`host`): up to six folders in a "My files" window on the right.
     * The left half stays clear for the sandbox (`sandboxIn`); slot 4 is the folder nearest it.
     */
    function scrDesktop(c: Computer, names: string[]): FileIcon[] {
        c.screen.replaceChildren();
        el("rect", { class: "scr", x: -c.w, y: c.top, width: 2 * c.w, height: c.bottom - c.top, rx: 4 }, c.screen);
        el("rect", { class: "ink thin", x: 40, y: -104, width: 165, height: 190, rx: 6 }, c.screen);
        el("rect", { class: "scrbar", x: 40, y: -104, width: 165, height: 16, rx: 6 }, c.screen);
        el("text", { class: "tiny", x: 48, y: -93 }, c.screen, "My files");
        return names.map((name, i) => folder(c.screen, name, ...deskSlot(i)));
    }

    /** A document icon (a file or program, not a folder); works with pop, wreck and lockFile like a folder. */
    function scrDoc(c: Computer, name: string, [x, y]: [number, number], cls?: string): FileIcon {
        const f = el("g", { class: cls ? "file doc " + cls : "file doc" }, c.screen);
        el("path", { d: `M${x - 11} ${y - 14} h14 l8 8 v20 h-22 Z` }, f);
        el("path", { class: "corner", d: `M${x + 3} ${y - 14} v8 h8 Z` }, f);
        el("text", { x, y: y + 27, "text-anchor": "middle" }, f, name);
        return Object.assign(f, { x, y });
    }

    /** A padlock on a folder's corner: the sandbox cannot reach it. */
    function lockFile(f: FileIcon) {
        const g = el("g", { class: "lock" }, f);
        el("path", { d: `M${f.x + 10} ${f.y + 2} v-4 a4 4 0 0 1 8 0 v4` }, g);
        el("rect", { x: f.x + 7, y: f.y + 2, width: 14, height: 11, rx: 2 }, g);
        pop(g);
    }

    /** Delete the given folders one by one with a red flash. */
    async function wreck(c: Computer, files: FileIcon[]) {
        for (const f of files) {
            f.classList.add("gone");
            el("rect", { class: "flash", x: -c.w, y: c.top, width: 2 * c.w, height: c.bottom - c.top }, c.screen);
            await sleep(650);
        }
    }

    return {
        scr, scrMsg, scrSettings, cursor, clickThrough, scrTerm, termType, termPrint,
        scrFiles, scrDesktop, deskSlot, scrDoc, lockFile, wreck,
    };
}
