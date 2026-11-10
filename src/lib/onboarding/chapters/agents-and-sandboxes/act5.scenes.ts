// Act 5 · Beyond the sandbox: one layer of many, and one way an agent can still get out.
// The recap scene lives here too: it shares the last page's arrangement, so the two read as one.
// TODO(i18n): every caption and label in this file.
import type { FileIcon, Placement, Scene } from "../../stage";
import { MY_FILES, SHARED } from "./copy";

// ----- layers of defence, email then agents -----

const ROWS = [185, 245, 305];
const THREAT_W = 140;
const GUARD_W = 150;
const EMAIL_L = 80;
const AGENT_L = 572;

export const layersOfDefence: Scene = ({ el, sleep, cap, pop, drawBrain, layers }) => {
    /** A fixed-width pill with its left edge at x, centred on y. */
    function tag(x: number, y: number, w: number, text: string, cls: "threat" | "guard" | "todo") {
        const g = el("g", { class: "tag " + cls }, layers.fx);
        el("rect", { x, y: y - 15, width: w, height: 30, rx: 15 }, g);
        el("text", { x: x + w / 2, y: y + 5, "text-anchor": "middle" }, g, text);
        pop(g);
    }

    /** One row: a misuse, then an arrow to the defence against it. */
    const threat = (L: number, row: number, text: string) => tag(L, ROWS[row], THREAT_W, text, "threat");
    function guard(L: number, row: number, text: string, cls: "guard" | "todo" = "guard") {
        const y = ROWS[row];
        const x = L + THREAT_W + 10;
        pop(el("path", { class: "arrow", d: `M${x} ${y} h26 m-8 -7 l8 7 l-8 7` }, layers.fx));
        tag(x + 36, y, GUARD_W, text, cls);
    }

    /** A dashed panel with an icon, a title and a one-line purpose; pills go in its rows. */
    function panel(L: number, title: string, purpose: string, drawIcon: (g: SVGGElement, cx: number, cy: number) => void) {
        const g = el("g", {}, layers.fx);
        el("rect", { class: "group", x: L - 22, y: 65, width: 390, height: 280, rx: 16 }, g);
        drawIcon(g, L + 20, 105);
        el("text", { class: "result", x: L + 54, y: 115 }, g, title);
        el("text", { class: "result-sub", x: L, y: 149 }, g, purpose);
        pop(g);
    }

    function envelope(parent: SVGGElement, cx: number, cy: number) {
        el("rect", { class: "ico", x: cx - 20, y: cy - 14, width: 40, height: 28, rx: 3 }, parent);
        el("path", { class: "ico line", d: `M${cx - 20} ${cy - 14} L${cx} ${cy + 2} L${cx + 20} ${cy - 14}` }, parent);
    }

    function smallBrain(parent: SVGGElement, cx: number, cy: number) {
        drawBrain(el("g", { transform: `translate(${cx} ${cy}) scale(0.34)` }, parent));
    }

    return {
        layout: {},
        async play() {
            await sleep(600);
            panel(EMAIL_L, "Email", "Built for one job: sending letters", envelope);
            cap("Email was built for one simple job: sending letters.");
            await sleep(2200);
            cap("Then people misused it.");
            for (const [i, text] of ["Spam", "Phishing", "Fraud"].entries()) {
                threat(EMAIL_L, i, text);
                await sleep(500);
            }
            await sleep(1200);
            cap("So defences were added, one layer at a time.");
            for (const [i, text] of ["Spam filters", "Warning labels", "Sender checks"].entries()) {
                guard(EMAIL_L, i, text);
                await sleep(600);
            }
            await sleep(2400);
            panel(AGENT_L, "Agents", "Built for one job: doing what you ask", smallBrain);
            cap("Agents are near the start of the same journey.");
            await sleep(2200);
            threat(AGENT_L, 0, "Wrong actions");
            guard(AGENT_L, 0, "Sandbox");
            cap("The sandbox is one layer.");
            await sleep(2200);
            threat(AGENT_L, 1, "Tricking you");
            guard(AGENT_L, 1, "Your judgement");
            cap("Your own care is another.");
            await sleep(2200);
            threat(AGENT_L, 2, "New tricks");
            guard(AGENT_L, 2, "?", "todo");
            cap("More will come, just as they did for email.");
        },
    };
};

// ----- the whole cast, with the sandbox on the host's screen -----

const HOST: Placement = [772, 228, 0.88];
const DESK = [...MY_FILES, SHARED];

export const outThroughYou: Scene = ({ sleep, tween, cap, pop, at, send, chatSay, host, box, scrDesktop, scrDoc, deskSlot, scrTerm, termType, termPrint, cursor, wreck, lockFile, link, reach, typing, keys, armsOver, jolt, sandboxIn }) => {
    let mine: FileIcon[] = [];
    return {
        layout: { you: [65, 240, 0.85], chat: [220, 228, 0.75], brain: [430, 228, 0.8], host: HOST, box: sandboxIn(HOST) },
        setup() {
            mine = scrDesktop(host, DESK);
            mine[MY_FILES.length].classList.add("shared");
            mine.slice(0, -1).forEach(lockFile);
            scrTerm(box, SHARED)?.classList.add("shared");
            armsOver();
        },
        async play() {
            const shared = mine[MY_FILES.length];
            await sleep(1000);
            link(at("host", 62, 43), at("box", 90, -62));
            chatSay("me", ["Build me a", "birthday website"]);
            await reach(keys("box"), 800);
            cap("The agent gets to work in the sandbox, as usual.");
            await send(at("brain", 60, 10), at("box", -50, -14), "commands", "cmd");
            typing(true);
            await termType(box, "make click-me.exe");
            termPrint(box, "✓ saved in Website", "ok");
            typing(false);
            await sleep(600);
            cap("It saves a program in the shared folder, so it is on your computer too.");
            await send(at("box", 60, -62), at("host", 82, 42), "click-me.exe", "shared", 900);
            pop(shared, "ping");
            // The free slot beside the shared folder, so the program reads as having come out of it.
            const exe = scrDoc(host, "click-me.exe", deskSlot(MY_FILES.length + 1), "shared");
            pop(exe);
            await sleep(2400);
            cap("Then the agent asks you to open it.");
            await send(at("brain", -62, 0), at("chat", 112, 0), "message", "reply");
            chatSay("ai", ["Done! Open click-me.exe", "on your computer to", "see your site."]);
            await sleep(2600);
            cap("It sounds helpful, so you open it on your real computer…");
            const pointer = cursor(host);
            const from = [210, 96];
            const to = [exe.x + 2, exe.y];
            await tween(900, (t) =>
                pointer.setAttribute("transform", `translate(${from[0] + (to[0] - from[0]) * t} ${from[1] + (to[1] - from[1]) * t})`),
            );
            exe.classList.add("hot");
            await sleep(600);
            pop(exe, "ping");
            await sleep(500);
            pointer.remove();
            exe.classList.remove("hot");
            cap("…and it runs outside the sandbox, where the locks never applied.");
            await wreck(host, mine.slice(0, -1));
            jolt("host", "shake");
            await sleep(2400);
            cap("The sandbox held. The program got out through you.");
            await sleep(2800);
            cap("So think twice before opening anything an agent hands you.");
        },
    };
};

export const wholePicture: Scene = ({ sleep, cap, think, send, at, chatSay, pop, host, box, scrDesktop, scrTerm, termType, termPrint, lockFile, link, reach, typing, keys, armsOver, sandboxIn }) => {
    let mine: FileIcon[] = [];
    return {
        layout: { you: [65, 240, 0.85], chat: [220, 228, 0.75], brain: [430, 228, 0.8], host: HOST, box: sandboxIn(HOST) },
        setup() {
            mine = scrDesktop(host, DESK);
            mine[DESK.length - 1].classList.add("shared");
            mine.slice(0, -1).forEach(lockFile);
            scrTerm(box, SHARED)?.classList.add("shared");
            armsOver();
        },
        async play() {
            await sleep(1000);
            // From the shared folder to the terminal's title bar: the agent is working inside that folder.
            link(at("host", 62, 43), at("box", 90, -62));
            await reach(keys("box"), 800);
            cap("You chat.");
            chatSay("me", ["Build me a", "birthday website"]);
            await sleep(1300);
            await send(at("chat", 112, -30), at("brain", -62, 0), "request");
            cap("The brain thinks.");
            think(true);
            await sleep(1400);
            think(false);
            cap("The arms do the work, safely inside the sandbox.");
            await send(at("brain", 60, 10), at("box", -50, -14), "commands", "cmd");
            typing(true);
            await termType(box, "build website");
            termPrint(box, "creating pages…");
            await sleep(800);
            termPrint(box, "✓ 12 files created", "ok");
            typing(false);
            await send(at("box", 60, -62), at("host", 82, 42), "changes", "shared", 800);
            pop(mine[DESK.length - 1], "ping");
            await send(at("box", -100, -14), at("brain", 62, 10), "results", "reply");
            await send(at("brain", -62, 0), at("chat", 112, 0), "done", "reply");
            chatSay("ai", ["Your site is ready!"]);
            cap("The website lands in your shared folder. Everything else stays out of reach.");
        },
    };
};
