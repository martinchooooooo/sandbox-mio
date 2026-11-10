// Act 3 · Out in the world: many brains, many arms, many tools, and why that makes them hard to compare.
// TODO(i18n): every caption and label in this file except the product names.
import type { Scene, StoryStage, ToolKind } from "../../stage";
import { HARNESSES, MAKERS, OPEN, TIED, type Harness, type Maker } from "./copy";

interface Brain {
    name: string;
    look: Maker;
}

const BRAINS: Brain[] = MAKERS.flatMap((m) => m.models.map((name) => ({ name, look: m })));

/** Whether harness `h` can drive `brain`; either may be missing while nothing is picked yet. */
const fits = (h: Harness | undefined, brain: Brain | undefined) =>
    !h || !brain || !h.onlyFrom || brain.look.name === h.onlyFrom;

// ----- the lineup of brains -----

const SLOT = 92; // width per small brain, wide enough for its name
const SCALE = 0.42; // small brain size relative to the big one
const ROWS = [112, 292]; // y of each row of small brains

interface Group {
    name: string;
    models: string[];
    /** The maker's look; missing for the "…and more" ghost. */
    look: Maker | null;
    w: number;
    x0: number;
    y: number;
    xs: number[];
}

/** Makers split into two rows with about the same number of brains, each row centred, so the lineup adapts when MAKERS changes. */
function lineup(): Group[] {
    const groups: Group[] = [
        ...MAKERS.map((m) => ({ name: m.name, models: m.models, look: m as Maker | null })),
        { name: "…and more", models: ["?"], look: null },
    ].map((g) => ({ ...g, w: g.models.length * SLOT + 16, x0: 0, y: 0, xs: [] }));
    const half = Math.ceil(groups.reduce((n, g) => n + g.models.length, 0) / 2);
    let split = 0;
    for (let slots = 0; split < groups.length && slots + groups[split].models.length <= half; split++)
        slots += groups[split].models.length;
    split = Math.max(1, split);
    const gap = 18;
    [groups.slice(0, split), groups.slice(split)].forEach((row, r) => {
        let x = 500 - (row.reduce((sum, g) => sum + g.w, 0) + gap * (row.length - 1)) / 2;
        for (const g of row) {
            g.x0 = x;
            g.y = ROWS[r];
            g.xs = g.models.map((_, i) => x + 8 + SLOT * (i + 0.5));
            x += g.w + gap;
        }
    });
    return groups;
}

export const manyBrains: Scene = ({ el, sleep, tween, cap, place, drawBrain, layers }) => {
    /** A small brain flies out of the big one's spot to (x, y); returns its pulsing group, or null for the ghost. */
    async function flyOut(name: string, x: number, y: number, look: Maker | null) {
        const g = el("g", { class: look ? "" : "ghost" }, layers.fx);
        const inner = el("g", { class: "inner" }, g);
        drawBrain(inner, look);
        await tween(650, (t) =>
            g.setAttribute("transform", `translate(${500 + (x - 500) * t} ${215 + (y - 215) * t}) scale(${0.2 + (SCALE - 0.2) * t})`),
        );
        if (look) el("text", { class: "name rise", x, y: y + 42, "text-anchor": "middle" }, layers.fx, name);
        else el("text", { class: "qbig rise", x, y: y + 10, "text-anchor": "middle" }, layers.fx, "?");
        return look ? inner : null;
    }

    return {
        layout: { brain: [500, 215, 1.1] },
        async play() {
            await sleep(900);
            cap("There isn't just one brain.");
            await sleep(1600);
            place("brain", [500, 215, 0.3, 0]);
            cap("Different companies build their own, and give them names.");
            // Groups start a beat apart without waiting for each other, so the lineup cascades out.
            const flights: Promise<SVGGElement | null>[] = [];
            for (const g of lineup()) {
                el("rect", { class: "group rise", x: g.x0, y: g.y - 46, width: g.w, height: 98, rx: 12 }, layers.fx);
                el("text", { class: "maker rise", x: g.x0 + g.w / 2, y: g.y - 54, "text-anchor": "middle" }, layers.fx, g.name);
                flights.push(...g.models.map((name, i) => flyOut(name, g.xs[i], g.y, g.look)));
                await sleep(300);
            }
            const minis = (await Promise.all(flights)).filter((m) => m !== null);
            await sleep(1200);
            cap("Some are quicker, some think more carefully, some cost less.");
            // Different pulse speeds hint that each brain thinks at its own pace.
            minis.forEach((m, i) => {
                m.style.animationDuration = `${0.3 + (i % 3) * 0.22}s`;
                m.classList.add("thinking");
            });
            await sleep(3400);
            minis.forEach((m) => m.classList.remove("thinking"));
            cap("But they all work the same way: words in, words out.");
        },
    };
};

// ----- harness name pills -----

export const manyNames: Scene = (s) => {
    const { el, sleep, cap, think, at, actors, jolt, pc, scrTerm, reach, typing, keys, relabel, armsName, harnessMark, paintBrain, isDark, layers } = s;

    /** A harness pill with its badge; the 'on' class fills it with the harness's colour. */
    function pill(h: Harness, cx: number, cy: number) {
        const g = el("g", { class: "pill rise" + (isDark(h.fill) ? " dark" : ""), style: `--on:${h.fill}` }, layers.fx);
        el("rect", { x: cx - 78, y: cy - 17, width: 156, height: 34, rx: 17 }, g);
        harnessMark(g, h, cx - 59, cy, 12);
        el("text", { x: cx + 12, y: cy + 5, "text-anchor": "middle" }, g, h.name);
        return g;
    }

    return {
        layout: { brain: [180, 225, 0.85], pc: [560, 205, 1.05] },
        setup() {
            scrTerm(pc);
        },
        async play() {
            await sleep(900);
            await reach(keys("pc"), 900);
            cap("Out in the world, the arms go by many names.");
            const words: SVGTextElement[] = [];
            const names: [string, number, number][] = [
                ["agent harness", 330, 80], ["coding agent", 590, 62], ["AI agent", 330, 395], ["harness", 590, 395],
            ];
            for (const [w, x, y] of names) {
                words.push(el("text", { class: "word", x, y, "text-anchor": "middle" }, layers.fx, w));
                await sleep(550);
            }
            await sleep(1400);
            cap("In this story, we simply call them the agent.");
            words.forEach((w) => w.classList.add("out"));
            armsName("Agent · the arms");
            await sleep(2400);

            cap("Different companies build different arms.");
            el("text", { class: "maker rise", x: 865, y: 76, "text-anchor": "middle" }, layers.fx, "Agent harnesses");
            const pills = new Map<Harness, SVGGElement>();
            const pick = (h: Harness) => {
                pills.forEach((p) => p.classList.remove("on"));
                pills.get(h)?.classList.add("on");
                armsName(h.name, h);
            };
            const swapBrain = (m: Maker, model = m.models[0]) => {
                relabel("brain", `Model · ${model}`);
                paintBrain(actors.brain.inner, m);
            };
            for (const [i, h] of HARNESSES.entries()) {
                pills.set(h, pill(h, 865, 112 + i * 48));
                pick(h);
                typing(true);
                await sleep(900);
                typing(false);
                await sleep(250);
            }
            await sleep(600);

            cap("Some only work with one company's brains.");
            pick(TIED);
            swapBrain(MAKERS.find((m) => m.name === TIED.onlyFrom)!);
            think(true);
            typing(true);
            await sleep(1800);
            think(false);
            typing(false);
            cap(`${TIED.name} can't use a brain from anyone but ${TIED.onlyFrom}.`);
            swapBrain(MAKERS.find((m) => m.name !== TIED.onlyFrom)!);
            await reach(at("brain", 95, 0), 600);
            jolt("brain", "shake");
            const no = el("text", { class: "nofit rise", x: at("brain", 160, 0)[0], y: at("brain", 0, 14)[1], "text-anchor": "middle" }, layers.fx, "✕");
            await sleep(2600);
            no.remove();

            cap(`Others, like ${OPEN.name}, take any brain you like.`);
            pick(OPEN);
            await reach(keys("pc"), 700);
            // Every other maker keeps the beat short while still showing a spread of brains.
            for (const m of MAKERS.filter((_, i) => i % 2 === 0)) {
                swapBrain(m);
                think(true);
                typing(true);
                await sleep(1300);
                think(false);
                typing(false);
                await sleep(300);
            }
        },
    };
};

// ----- the toolbox -----

const KIT: [ToolKind, string][] = [
    ["terminal", "Terminal"], ["read", "Read file"], ["edit", "Edit file"], ["find", "Find files"],
    ["web", "Web search"], ["browser", "Browser"], ["todo", "To-do list"], ["image", "Images"],
    ["memory", "Memory"], ["email", "Email"], ["calendar", "Calendar"], ["ask", "Ask you"],
];
const tileAt = (i: number): [number, number] => [345 + (i % 4) * 80, 110 + Math.floor(i / 4) * 90];
// [tile index, caption, what the tool does on the computer]
const JOBS: [number, string, string][] = [
    [1, "Read a file.", "read notes.txt"],
    [4, "Search the web.", 'search "cake recipes"'],
    [2, "Edit a file.", "edit shopping-list"],
    [0, "Run a command.", "build website"],
];

export const armsCarryTools: Scene = ({ sleep, cap, send, at, pc, scrTerm, termPrint, reach, typing, tool, layers }) => ({
    layout: { brain: [160, 235, 0.8], pc: [820, 205, 0.95] },
    setup() {
        scrTerm(pc);
    },
    async play() {
        await sleep(800);
        cap("The arms don't work bare-handed. They pick up tools.");
        // Tiles go behind the arms layer so the hands show on top of the tool they hold.
        const tiles: SVGGElement[] = [];
        for (let i = 0; i < 6; i++) {
            tiles.push(tool(layers.fxBack, ...KIT[i], ...tileAt(i)));
            await sleep(220);
        }
        await sleep(1200);
        cap("The brain picks the tool. The arms use it.");
        await sleep(1600);
        for (const [i, line, job] of JOBS) {
            const [x, y] = tileAt(i);
            await reach([x, y], 650);
            tiles[i].classList.add("hot");
            cap(line);
            typing(true);
            await send([x + 30, y], at("pc", -60, -20), job, "cmd", 800);
            typing(false);
            termPrint(pc, "✓ " + job, "ok");
            tiles[i].classList.replace("hot", "used");
            await sleep(700);
        }
        tiles.forEach((t) => t.classList.remove("used"));

        cap("Some agents carry just a handful of tools.");
        tiles.slice(3).forEach((t) => t.classList.add("dim"));
        await sleep(2600);
        cap("Others carry dozens, and many let you add your own.");
        tiles.forEach((t) => t.classList.remove("dim"));
        for (let i = 6; i < KIT.length; i++) {
            tiles.push(tool(layers.fxBack, ...KIT[i], ...tileAt(i)));
            await sleep(200);
        }
    },
});

// ----- the combination cards -----

interface Card {
    g: SVGGElement;
    val: SVGTextElement;
    icon: SVGGElement;
    options: string[];
    /** Runs on every tick of a spin, e.g. to recolour the brain icon. */
    onPick?: (index: number, icon: SVGGElement) => void;
    /** Rejects options that would make an impossible pairing. */
    ok?: (index: number) => boolean;
    i: number;
}

type CardSpec = [string, (g: SVGGElement) => void, string[], Card["onPick"]?];

function cardKit({ el, sleep, pop, harnessMark, layers }: StoryStage) {
    /** The Arms card icon: one arm whose hand is the harness badge, or a plain hand before any harness is picked. */
    function drawArmIcon(g: SVGGElement, h: Harness | null) {
        g.replaceChildren();
        const d = "M-34 18 Q-6 -28 22 -8";
        el("path", { class: "arm-out", d }, g);
        const inside = el("path", { class: "arm-in", d }, g);
        if (!h) {
            el("circle", { class: "hand", cx: 22, cy: -8, r: 9 }, g);
            return;
        }
        inside.style.stroke = h.fill;
        harnessMark(g, h, 24, -10, 16);
    }

    function card(i: number, [head, drawIcon, options, onPick]: CardSpec): Card {
        const w = 190;
        const g = el("g", { class: "card", transform: `translate(${60 + i * 230} 70)` }, layers.fx);
        const body = el("g", { class: "rise" }, g);
        el("rect", { class: "ink frame", width: w, height: 170, rx: 14 }, body);
        el("path", { class: "ink fill-accent", d: `M0 34 V14 a14 14 0 0 1 14 -14 H${w - 14} a14 14 0 0 1 14 14 V34 Z` }, body);
        el("text", { class: "head", x: w / 2, y: 23, "text-anchor": "middle" }, body, head);
        const icon = el("g", { transform: `translate(${w / 2} 82)` }, body);
        drawIcon(icon);
        const val = el("text", { class: "val wait", x: w / 2, y: 148, "text-anchor": "middle" }, body, "?");
        return { g, val, icon, options, onPick, i: -1 };
    }

    /** Flick through a card's options like a slot machine, landing `ticks` steps on and skipping any option `c.ok` rejects. */
    async function spin(c: Card, ticks: number) {
        c.g.classList.add("spin");
        c.val.classList.remove("wait");
        for (let k = 0; k < ticks; k++) {
            do c.i = (c.i + 1) % c.options.length;
            while (c.ok && !c.ok(c.i));
            c.val.textContent = c.options[c.i];
            c.onPick?.(c.i, c.icon);
            await sleep(60 + k * 14);
        }
        c.g.classList.remove("spin");
        pop(c.val);
    }

    return { drawArmIcon, card, spin };
}

export const hardToCompare: Scene = (s) => {
    const { el, sleep, cap, pop, drawBrain, paintBrain, tool, layers } = s;
    const { drawArmIcon, card, spin } = cardKit(s);

    return {
        layout: {},
        async play() {
            const fx = layers.fx;
            await sleep(800);
            cap("What you actually use is a mix of parts.");
            const specs: CardSpec[] = [
                ["Brain", (g) => drawBrain(el("g", { transform: "scale(0.4)" }, g)), BRAINS.map((b) => b.name),
                    (i, icon) => paintBrain(icon, BRAINS[i].look)],
                ["Arms", (g) => drawArmIcon(g, null), HARNESSES.map((h) => h.name),
                    (i, icon) => drawArmIcon(icon, HARNESSES[i])],
                ["Tools", (g) => {
                    tool(g, "read", "", -16, 6);
                    tool(g, "terminal", "", 16, -6);
                }, ["4 tools", "12 tools", "30+ tools", "7 tools", "20 tools"]],
                ["And more", (g) => [-18, 0, 18].forEach((cx) => el("circle", { class: "pip", cx, cy: 0, r: 6 }, g)),
                    ["your instructions", "its settings", "how you ask", "the task itself"]],
            ];
            const cards: Card[] = [];
            for (const [i, spec] of specs.entries()) {
                if (i > 0) el("text", { class: "plus rise", x: 40 + i * 230, y: 168, "text-anchor": "middle" }, fx, "+");
                cards.push(card(i, spec));
                await sleep(400);
            }
            // Brain and arms must stay a real pairing, e.g. never Claude Code with a non-Anthropic brain.
            const [brainCard, armsCard] = cards;
            brainCard.ok = (i) => fits(HARNESSES[armsCard.i], BRAINS[i]);
            armsCard.ok = (i) => fits(HARNESSES[i], BRAINS[brainCard.i]);
            const result = el("text", { class: "result", x: 500, y: 318, "text-anchor": "middle" }, fx);
            const sub = el("text", { class: "result-sub", x: 500, y: 356, "text-anchor": "middle" }, fx);
            await sleep(900);

            cap("A brain…");
            await spin(cards[0], 10);
            cap("…in a set of arms…");
            await spin(cards[1], 9);
            cap("…carrying some tools…");
            await spin(cards[2], 7);
            cap("…plus its instructions, settings, and how you ask.");
            await spin(cards[3], 8);
            await sleep(500);
            result.textContent = "= one particular agent";
            pop(result);
            cap("Together, they make one particular agent.");
            await sleep(2400);

            cap("Change any one part, and you get a different agent.");
            const swaps: [number, string][] = [[1, "= a different agent"], [0, "= a different agent again"], [2, "= yet another agent"]];
            for (const [k, text] of swaps) {
                await spin(cards[k], 6 + k);
                result.textContent = text;
                pop(result);
                await sleep(1400);
            }

            cap('So "which AI is best?" rarely has a simple answer.');
            result.textContent = "= hundreds of possible agents";
            pop(result);
            const pairs = HARNESSES.reduce((n, h) => n + BRAINS.filter((b) => fits(h, b)).length, 0);
            sub.textContent = `Just the ${BRAINS.length} brains and ${HARNESSES.length} harnesses in this story make ${pairs} working pairs, before tools or settings.`;
            pop(sub);
        },
    };
};
