// The agent, drawn as two arms growing from the brain to a moving pair of hands.
// TODO(i18n): the arms' default label.
import type { Board } from "./board";
import { isDark, type Look } from "./characters";
import type { Computers } from "./computers";
import type { Core } from "./core";
import type { Tools, ToolKind } from "./tools";

/** A harness badge: brand colour plus a monogram standing in for the real logo. */
export interface Mark extends Look {
    mark: string;
}

const ARM_LABEL = "Agent · the arms";

export function createArms(core: Core, board: Board, computers: Computers, tools: Tools) {
    const { el, tween } = core;
    const { at, pop, pos, layers, svg } = board;
    const armsG = layers.arms;

    const armEls = [0, 1].map(() => [el("path", { class: "arm-out" }, armsG), el("path", { class: "arm-in" }, armsG)]);
    const hands = [el("circle", { class: "hand", r: 9 }, armsG), el("circle", { class: "hand b", r: 9 }, armsG)];
    const armLabel = el("text", { class: "label", "text-anchor": "middle" }, armsG, ARM_LABEL);
    const arm = { x: 0, y: 0, shown: false };
    armsG.style.opacity = "0";

    /** Tiles carried by the hands and the "+N" count for calls beyond two. */
    let held: SVGGElement[] = [];
    const countEl = el("text", { class: "label", "text-anchor": "middle" }, layers.fxBack);

    function brainPos(): [number, number, number] {
        const p = pos.brain;
        if (!p) throw new Error("arms need `brain` in the page's layout");
        return p;
    }

    // Redrawn from the brain's current placement every frame, so the arms follow it.
    function drawArms() {
        const [bx, by, bs] = brainPos();
        const sx = bx + 40 * bs;
        [-1, 1].forEach((sign, i) => {
            const sy = by + sign * 32 * bs;
            const hy = arm.y + sign * 15;
            const d = `M${sx} ${sy} Q${(sx + arm.x) / 2} ${(sy + hy) / 2 + sign * 26} ${arm.x} ${hy}`;
            armEls[i].forEach((p) => p.setAttribute("d", d));
            hands[i].setAttribute("cx", String(arm.x));
            hands[i].setAttribute("cy", String(hy));
        });
        held.forEach((tile, i) => tile.setAttribute("transform", `translate(${arm.x + (i === 0 ? -34 : 34)} ${arm.y - 6})`));
        countEl.setAttribute("x", String(arm.x));
        countEl.setAttribute("y", String(arm.y + 52));
        armLabel.setAttribute("x", String((sx + arm.x) / 2));
        armLabel.setAttribute("y", String(Math.min(by - 32 * bs, arm.y - 15) - 26));
    }

    function ensureShown() {
        if (arm.shown) return;
        const [bx, by, bs] = brainPos();
        arm.x = bx + 50 * bs;
        arm.y = by;
        arm.shown = true;
        armsG.style.opacity = "1";
    }

    /** Extend the arms (first call) or move the hands to `target` in stage coordinates; `ms` 0 jumps there. */
    async function reach(target: [number, number], ms = 800) {
        ensureShown();
        if (ms <= 0) {
            [arm.x, arm.y] = target;
            drawArms();
            return;
        }
        const x0 = arm.x;
        const y0 = arm.y;
        await tween(ms, (t) => {
            arm.x = x0 + (target[0] - x0) * t;
            arm.y = y0 + (target[1] - y0) * t;
            drawArms();
        });
    }

    /** Draw the hands back to the brain, then hide the arms; `ms` 0 hides them within the same call. */
    async function retract(ms = 500) {
        if (!arm.shown) return;
        const reaching = reach(at("brain", 50, 0), ms);
        if (ms > 0) await reaching;
        arm.shown = false;
        armsG.style.opacity = "0";
    }

    /** Put one tool tile in each hand (the first two kinds) and keep them moving with the hands. */
    function carry(kinds: ToolKind[]) {
        held.forEach((tile) => tile.remove());
        held = kinds.slice(0, 2).map((kind) => tools.tool(layers.fxBack, kind, "", 0, 0));
        if (held.length > 0) {
            ensureShown();
            drawArms();
        }
    }

    /** A "+N" badge beside the hands for tool calls beyond the two they hold; 0 clears it. */
    function carryCount(extra: number) {
        countEl.textContent = extra > 0 ? `+${extra}` : "";
    }

    /** Toggle the hands' tapping animation. */
    const typing = (on: boolean) => hands.forEach((h) => h.classList.toggle("typing", on));

    /** Where the hands rest on a computer: the left end of its keyboard. */
    const keys = (name: string) => at(name, ...computers.computers[name].keys);

    /** Paint the arms above the computers for the current page; page turns put them back behind. */
    const armsOver = () => svg.insertBefore(armsG, layers.fx);

    function paintArms(look: Look | null | undefined) {
        armEls.forEach(([, inside]) => (inside.style.stroke = look ? look.fill : ""));
        hands.forEach((h) => (h.style.fill = look ? look.fill : ""));
    }

    /** Rename the arms for the current page, optionally in a harness's colour; page turns restore both. */
    function armsName(text: string, look?: Look | null) {
        armLabel.textContent = text;
        pop(armLabel);
        paintArms(look);
    }

    /** A harness badge: a roundel in its colour with its monogram, centred on (x, y). */
    function harnessMark(parent: Element, h: Mark, x: number, y: number, r = 11): SVGGElement {
        const g = el("g", { class: "mark" }, parent);
        el("circle", { cx: x, cy: y, r, style: `fill:${h.fill}` }, g);
        const t = el("text", { x, y: y + r * 0.34, "text-anchor": "middle", style: `font-size:${r * 0.95}px` }, g, h.mark);
        if (isDark(h.fill)) t.style.fill = "#fff";
        return g;
    }

    core.resets.push(() => {
        held = [];
        countEl.textContent = "";
        layers.fxBack.append(countEl);
        svg.insertBefore(armsG, layers.actors);
        arm.shown = false;
        armsG.style.opacity = "0";
        armLabel.textContent = ARM_LABEL;
        paintArms(null);
        typing(false);
    });

    return { reach, retract, carry, carryCount, typing, keys, armsOver, armsName, harnessMark };
}
