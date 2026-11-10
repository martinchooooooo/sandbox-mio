// Act 2 · Then: the brain gets arms (the agent) and drives the computer through a terminal.
// TODO(i18n): every caption and label in this file.
import type { Scene, ScreenButton } from "../../stage";

export const armsAppear: Scene = ({ sleep, cap, pc, scrSettings, reach, typing, keys }) => ({
    layout: { you: [80, 215, 0.8, 0.35], chat: [240, 205, 0.7, 0.35], brain: [500, 205], pc: [830, 200] },
    setup() {
        scrSettings(pc);
    },
    async play() {
        await sleep(1000);
        cap("What if the brain could reach the computer itself?");
        await sleep(1900);
        cap("So we gave it arms.");
        await reach(keys("pc"), 1500);
        await sleep(500);
        cap("The arms are called an agent. They do the work for you.");
        typing(true);
        await sleep(1800);
        typing(false);
    },
});

export const mouseVsTerminal: Scene = ({ sleep, cap, pc, scrSettings, scrTerm, termType, termPrint, clickThrough, reach, typing, keys }) => {
    let btns: ScreenButton[] = [];
    return {
        layout: { brain: [230, 215, 0.9], pc: [690, 190, 1.45] },
        setup() {
            btns = scrSettings(pc);
        },
        async play() {
            await sleep(1000);
            cap("You use a computer by pointing and clicking.");
            await clickThrough(pc, btns, true);
            await sleep(600);
            cap("An agent uses a terminal: it types commands instead.");
            scrTerm(pc);
            await reach(keys("pc"), 900);
            typing(true);
            await termType(pc, "passwd");
            termPrint(pc, "New password: ••••••••");
            await sleep(700);
            typing(false);
            termPrint(pc, "✓ password updated", "ok");
            cap("Same result. No mouse needed.");
        },
    };
};

export const theNewLoop: Scene = ({ sleep, cap, think, send, at, chatSay, pc, scrTerm, termType, termPrint, reach, typing, keys }) => ({
    layout: { you: [80, 215, 0.9], chat: [250, 205, 0.8], brain: [500, 205, 0.85], pc: [820, 200] },
    setup() {
        scrTerm(pc);
    },
    async play() {
        await sleep(1000);
        cap("1. You ask in the chat.");
        chatSay("me", ["Change my laptop", "password, please"]);
        await sleep(1300);
        cap("2. The agent carries your request to the brain.");
        await send(at("chat", 112, -30), at("brain", -62, 0), "request");
        think(true);
        cap("3. The brain decides what to do.");
        await sleep(1500);
        think(false);
        cap("4. Its reply is a command for the arms.");
        await reach(keys("pc"), 800);
        await send(at("brain", 60, -14), at("pc", -50, -14), "passwd", "cmd");
        typing(true);
        await termType(pc, "passwd");
        typing(false);
        termPrint(pc, "✓ password updated", "ok");
        await sleep(500);
        cap("5. The arms report back what happened.");
        await send(at("pc", -100, -14), at("brain", 62, -14), "it worked ✓", "reply");
        think(true);
        await sleep(800);
        think(false);
        cap("6. And you get the good news.");
        await send(at("brain", -62, 0), at("chat", 112, 0), "done", "reply");
        chatSay("ai", ["Done! Your password", "has been changed."]);
    },
});
