// Act 1 · At first: the brain, the chat, and doing the work yourself.
// TODO(i18n): every caption and label in this file.
import type { Scene, ScreenButton } from "../../stage";
import { QUESTION, STEPS } from "./copy";

export const meetTheBrain: Scene = ({ el, sleep, cap, think, layers }) => ({
    layout: { brain: [500, 195, 1.25] },
    async play() {
        cap("This is a model. Think of it as a brain.");
        await sleep(1700);
        think(true);
        cap("Give it words, and it gives words back.");
        const words: [string, number, number][] = [
            ["answers", 290, 120], ["stories", 715, 125], ["recipes", 275, 285], ["advice", 725, 280], ["code", 500, 60],
        ];
        for (const [w, x, y] of words) {
            el("text", { class: "word", x, y, "text-anchor": "middle" }, layers.fx, w);
            await sleep(500);
        }
        think(false);
        await sleep(600);
        cap("It can think and talk. It cannot touch anything.");
    },
});

export const chatAndReply: Scene = ({ sleep, cap, think, send, at, chatSay }) => ({
    layout: { you: [110, 210], chat: [340, 205], brain: [720, 205] },
    async play() {
        await sleep(900);
        cap("You type a question into the chat.");
        chatSay("me", QUESTION);
        await sleep(1500);
        cap("The chat sends your words to the brain.");
        await send(at("chat", 112, -40), at("brain", -62, 0), "your question");
        think(true);
        cap("The brain thinks…");
        await sleep(1600);
        think(false);
        cap("…and writes a reply. Just text.");
        await send(at("brain", -62, 0), at("chat", 112, 0), "its answer", "reply");
        chatSay("ai", STEPS);
    },
});

export const clickAlong: Scene = ({ el, sleep, cap, chatSay, pc, scrSettings, scrMsg, clickThrough, layers, AUTO }) => {
    let btns: ScreenButton[] = [];
    return {
        layout: { you: [90, 215], chat: [285, 205, 0.85], brain: [520, 205, 0.7, 0.4], pc: [820, 200] },
        setup() {
            chatSay("me", QUESTION);
            chatSay("ai", STEPS);
            btns = scrSettings(pc);
        },
        async play() {
            await sleep(900);
            el("path", { class: "guide", d: "M100 290 C220 405 600 405 700 270" }, layers.fxBack);
            cap(AUTO ? "The brain gave instructions. You do the clicking." : "Your turn: click the glowing buttons on the computer.");
            await clickThrough(pc, btns, AUTO);
            scrMsg(pc, "Your laptop", "Password changed ✓");
            cap("Done. The brain did the thinking. You did the work.");
        },
    };
};
