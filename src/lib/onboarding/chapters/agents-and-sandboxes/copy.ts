// Copy reused across several pages of this chapter, so a wording change happens in one place.
// TODO(i18n): every string in this file except the product names.
import type { Look, Mark } from "../../stage";

export interface Maker extends Look {
    name: string;
    models: string[];
}

export interface Harness extends Mark {
    name: string;
    /** The one maker whose brains this harness accepts; the story never pairs it with any other. */
    onlyFrom?: string;
}

export const QUESTION = ["How do I change my", "laptop password?"];
export const STEPS = ["1. Open Settings", "2. Click Accounts", "3. Change password"];
export const MY_FILES = ["Photos", "Documents", "Music", "Taxes"];
export const BOX_FILES = ["project", "notes", "drafts", "scratch"];
/** The one folder shared into the sandbox; also what the agent builds there. */
export const SHARED = "Website";

// Real product names go out of date quickly, so they live only here; the pages lay out whatever
// is listed. `fill` and `folds` echo each maker's logo: the only brand colours in the story.
export const MAKERS: Maker[] = [
    { name: "Anthropic", fill: "#d97757", models: ["Opus", "Fable"] },
    { name: "OpenAI", fill: "#111111", models: ["Astra", "Luna"] },
    { name: "Google", fill: "#ffffff", folds: ["#4285f4", "#ea4335", "#fbbc05", "#34a853"], models: ["Gemini Pro", "Gemini Flash"] },
    { name: "Meta", fill: "#0866ff", models: ["Muse"] },
    { name: "DeepSeek", fill: "#4d6bfe", models: ["DeepSeek V4"] },
    { name: "Mistral", fill: "#fa520f", models: ["Mistral Large"] },
    { name: "Alibaba", fill: "#615ced", models: ["Qwen"] },
    { name: "NVIDIA", fill: "#76b900", models: ["Nemotron"] },
];

// Monogram badges stand in for the real logos, which are not redrawn.
export const HARNESSES: Harness[] = [
    { name: "Claude Code", fill: "#d97757", mark: "CC", onlyFrom: "Anthropic" },
    { name: "Codex", fill: "#111111", mark: "Cx" },
    { name: "GitHub Copilot", fill: "#8250df", mark: "GH" },
    { name: "OpenCode", fill: "#ffffff", mark: "OC" },
    { name: "Pi", fill: "#14b8a6", mark: "π" },
];

/** The harness tied to one maker, and an open one to contrast it with. */
export const TIED = HARNESSES.find((h) => h.onlyFrom)!;
export const OPEN = HARNESSES.filter((h) => !h.onlyFrom).at(-1)!;

/** ['a', 'b', 'c'] -> 'a, b and c', for writing the lists above into body text. */
export const andList = (xs: string[]) =>
    xs.length < 2 ? xs.join("") : `${xs.slice(0, -1).join(", ")} and ${xs[xs.length - 1]}`;
