import type { Component } from "svelte";
import { m } from "$lib/paraglide/messages.js";
import AgentsAndSandboxes from "./chapters/agents-and-sandboxes/AgentsAndSandboxes.svelte";

/** One chapter of the onboarding book: a title and a component made of `<Page>`s. */
export interface ChapterEntry {
    id: string;
    /** A function, so the title follows the active locale. */
    title: () => string;
    component: Component;
}

/**
 * Every chapter the onboarding book shows, in reading order. To add your own, create a component
 * under `chapters/<your-chapter>/` and append it here (`src/lib/onboarding/AGENTS.md`).
 */
export const chapters: ChapterEntry[] = [
    { id: "agents-and-sandboxes", title: m.onboarding_story_title, component: AgentsAndSandboxes },
];
