// Copy for the Models page. Technical names (llama.cpp, Vulkan, RAM, GGUF) stay out of visible text.
// TODO(i18n): every string in this file.
import type { Fit } from "./recommend";
import type { EngineKind, FitGroup, FitVerdict, GoodFor } from "./types";

export const MODEL_SUMMARIES: Record<string, string> = {
    "qwen3.5-2b": "The smallest and quickest. Fine for simple questions; makes more mistakes on bigger jobs.",
    "qwen3.5-4b": "Small and quick, and steadier than the 2B.",
    "qwen3.5-9b": "A good all-rounder for a computer with 16 GB of memory.",
    "gemma-4-12b-it": "Google's mid-sized open Model.",
    "gpt-oss-20b": "OpenAI's open Model that runs on your own computer.",
    "devstral-small-2-24b": "Mistral's Model made for coding agents; answers without thinking out loud.",
    "qwen3.8-27b": "The most capable Model on the shelf, and the slowest.",
    "qwen3.6-35b-a3b":
        "A very large Model that only wakes a small part of itself per word, so it answers faster than its size suggests. Biggest download.",
    "test-model": "A tiny Model used for testing.",
};

export const GOOD_FOR_LABELS: Record<GoodFor, string> = {
    "quick-replies": "Quick replies",
    "everyday-chat": "Everyday chat",
    "coding-agent": "Coding with an agent",
    "careful-thinking": "Careful thinking",
    "long-documents": "Long documents",
};

export const SIZE_LABELS = ["Small", "Medium", "Large", "Very large"];
export const SPEED_LABELS = ["Quick", "Steady", "Slow"];

export const FIT_LABELS: Record<FitVerdict, string> = {
    fits: "Runs well here",
    tight: "Runs slowly",
    busy: "Needs more free memory",
    "too-small": "Needs a bigger computer",
    engine: "Not on this engine",
};

export function fitSentence(f: Fit): string {
    switch (f.verdict) {
        case "fits": return "Runs well on this computer.";
        case "tight": return "Runs here, but slowly. Closing other apps helps.";
        case "busy": return `Needs about ${f.needGb} GB free; about ${f.freeGb} GB is free right now. Close some apps, then check again.`;
        case "too-small": return `Needs a computer with at least ${f.needGb} GB of memory. This one has ${f.totalGb} GB.`;
        case "engine": return "Not available with Microsoft Foundry Local. The built-in engine can run it.";
    }
}

export const GROUP_TITLES: Record<FitGroup, string> = {
    "runs-well": "Runs well here",
    "runs-slowly": "Runs here, slowly",
    "needs-more": "Needs more than this computer has",
};

// Plain titles of the engine-owned setup steps, mirroring `titles` in RuntimeSteps.svelte.
export const PLAIN_STEP_TITLES: Record<string, string> = {
    "detect-device": "Looking for a graphics chip that can help…",
    "engine-ready": "Getting the engine ready…",
    "model-ready": "Making sure the Model is on this computer…",
    "launch-server": "Bringing the Model into memory…",
    "health-check": "Checking it answers…",
    "check-foundry": "Looking for Microsoft Foundry Local…",
    "register-eps": "Finding the fastest chip to use…",
    "start-service": "Starting the service…",
    "load-model": "Bringing the Model into memory…",
    "resolve-id": "Checking it answers…",
};

export const EXPLAINERS = {
    model: {
        title: "What is a Model?",
        body: "A Model is the Brain from the tour: the part that reads your words and writes back. This one lives on your own computer, so nothing you ask leaves it.",
    },
    memory: {
        title: "Memory",
        body: "While a Model is awake it sits in your computer's memory. A bigger Model needs more room, and less is left for your other apps.",
    },
    download: {
        title: "Download size",
        body: "The Model is one big file you download once. The biggest ones are as large as a long film.",
    },
    speed: {
        title: "Expected speed",
        body: "An estimate from the Model's size and your graphics chip. A bigger Model has read more and makes fewer mistakes, but answers more slowly.",
    },
    size: {
        title: "Size",
        body: "A bigger Model has read more and makes fewer mistakes, but needs more memory and answers more slowly.",
    },
    pick: {
        title: "Why this one?",
        body: "It is the most capable Model that fits comfortably in the memory this computer has free.",
    },
    cantRun: {
        title: "Why can't this computer run it?",
        body: "A Model has to fit in memory while it works. This one needs more than this computer has, or more than is free right now.",
    },
};

export const ENGINE_NAMES: Record<EngineKind, string> = { llama: "llama.cpp with Vulkan", foundry: "Microsoft Foundry Local" };

export const PAGE = {
    title: "Models",
    intro: "A Model is the Brain your sandboxes' agents think with. Pick one to keep on this computer.",
    yourModel: "Your Model",
    yourComputer: "Your computer",
    otherModels: "Other Models",
    pickBadge: "Our pick for this computer",
    notDownloaded: "Not downloaded yet",
    downloading: (pct: number) => `Downloading… ${pct}%`,
    asleep: "Asleep",
    awake: "Awake: your sandboxes can use it",
    waking: "Waking it up…",
    ready: "Ready ✓",
    failed: "That did not go through.",
    downloaded: "Downloaded",
    downloadAndWake: "Download and wake up",
    wake: "Wake it up",
    sleep: "Put it to sleep",
    tryAnyway: "Try anyway",
    useThis: "Use this Model",
    downloadAnyway: "Download anyway",
    sandboxesUse: "Your sandboxes' agents use this Model.",
    openSandbox: "Open hello-world",
    computer: (totalGb: number, freeGb: number, gpu: boolean) =>
        `This computer has about ${totalGb} GB of memory, about ${freeGb} GB of it free right now, and ${gpu ? "a graphics chip that can help" : "no graphics chip that can help"}.`,
    wakeSteps: "What waking a Model involves with this engine",
    size: "Size",
    speed: "Expected speed",
    download: (gb: number) => `${gb} GB to download`,
    memory: (gb: number) => `about ${gb} GB of memory`,
};
