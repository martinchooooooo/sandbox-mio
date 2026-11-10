// Act 4 · Now: brains make mistakes, so the agent works in a sandbox on your computer, with one shared folder.
// TODO(i18n): every caption and label in this file.
import type { FileIcon, Placement, Scene } from "../../stage";
import { BOX_FILES, MY_FILES, SHARED } from "./copy";

// The shared folder goes last, into the desktop slot nearest the sandbox (see `scrDesktop`).
const DESK = [...MY_FILES, SHARED];
const HOST: Placement = [600, 228, 1.15];

export const mistakeOnRealPc: Scene = ({ sleep, cap, pc, scrFiles, wreck, reach, typing, keys, jolt }) => {
    let files: FileIcon[] = [];
    return {
        layout: { brain: [230, 215, 0.9], pc: [690, 190, 1.45] },
        setup() {
            files = scrFiles(pc, "My files", MY_FILES);
        },
        async play() {
            await sleep(1000);
            await reach(keys("pc"), 900);
            cap("The agent is working on your real computer…");
            typing(true);
            await sleep(1800);
            cap("…and the brain gets something wrong.");
            jolt("brain", "wobble");
            await sleep(700);
            await wreck(pc, files.slice(0, 2));
            typing(false);
            jolt("pc", "shake");
            cap("Oops. Those were your real files.");
        },
    };
};

export const sandboxAppears: Scene = ({ sleep, cap, place, actors, host, box, scrFiles, scrDesktop, wreck, setBadge, reach, typing, keys, armsOver, jolt, sandboxIn }) => {
    let files: FileIcon[] = [];
    return {
        layout: { brain: [185, 230, 0.95], host: HOST },
        setup() {
            scrDesktop(host, DESK);
            files = scrFiles(box, "Linux", BOX_FILES);
            armsOver();
            // Start the sandbox hidden but already on the screen, so it grows in rather than
            // sliding from the centre.
            const [x, y, s = 1] = sandboxIn(HOST);
            actors.box.g.style.transform = `translate(${x}px,${y}px) scale(${s * 0.7})`;
        },
        async play() {
            await sleep(1000);
            await reach(keys("host"), 800);
            cap("Arms on your real computer: risky.");
            await sleep(1900);
            cap("So we build a sandbox: a pretend computer inside your real one.");
            place("box", sandboxIn(HOST));
            await sleep(2200);
            cap("The agent works in there instead.");
            await reach(keys("box"), 1000);
            setBadge(host, "out of reach");
            typing(true);
            await sleep(1800);
            cap("If the brain gets something wrong…");
            jolt("brain", "wobble");
            await sleep(700);
            await wreck(box, files.slice(0, 2));
            typing(false);
            jolt("box", "shake");
            await sleep(900);
            cap("…the mess stays in the sandbox. Your files are fine.");
            setBadge(host, "✓ untouched");
            await sleep(2600);
            cap("And a broken sandbox can be thrown away and replaced.");
            scrFiles(box, "Linux", BOX_FILES, true);
        },
    };
};

export const sharingOneFolder: Scene = ({ sleep, cap, send, at, pop, host, box, scrFiles, scrDesktop, lockFile, link, reach, typing, keys, armsOver, sandboxIn }) => {
    let mine: FileIcon[] = [];
    let inside: FileIcon[] = [];
    return {
        layout: { brain: [185, 230, 0.95], host: HOST, box: sandboxIn(HOST) },
        setup() {
            mine = scrDesktop(host, DESK);
            inside = scrFiles(box, "Linux", [...BOX_FILES.slice(1), SHARED]);
            inside[3].style.opacity = "0";
            armsOver();
        },
        async play() {
            const outer = mine[DESK.length - 1];
            const copy = inside[3];
            await sleep(1000);
            cap("Your website lives in a folder on your real computer.");
            pop(outer, "ping");
            await sleep(2000);
            cap("Share it, and the same folder shows up inside the sandbox.");
            outer.classList.add("shared");
            // From the folder's left edge to its copy's right edge.
            link(at("host", 62, 43), at("box", 62, 17));
            await sleep(600);
            copy.style.opacity = "";
            copy.classList.add("shared");
            pop(copy);
            await sleep(2200);
            cap("The agent works on it in there…");
            await reach(keys("box"), 900);
            typing(true);
            await sleep(600);
            pop(copy, "ping");
            await sleep(900);
            typing(false);
            await send(at("box", 42, 16), at("host", 82, 42), "changes", "shared", 800);
            pop(outer, "ping");
            cap("…and you see the changes on your side too.");
            await sleep(2400);
            cap("Folders you did not share stay out of reach.");
            for (const f of mine.slice(0, -1)) {
                lockFile(f);
                await sleep(250);
            }
        },
    };
};
