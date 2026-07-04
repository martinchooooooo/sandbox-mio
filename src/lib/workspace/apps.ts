import { Home01Icon, TerminalIcon, BotIcon } from "@hugeicons/core-free-icons";
import { m } from "$lib/paraglide/messages.js";
import type { AppKind } from "./state.svelte";

export type LaunchContext = { installedAgents: unknown[] };

export type AppDescriptor = {
    kind: AppKind;
    title: () => string;
    icon: typeof Home01Icon;
    command: string;
    available(ctx: LaunchContext): boolean;
};

/** Manage/Home tab metadata. Not a catalogue entry: always present, not launchable, not closable. */
export const HOME_APP = {
    title: m.workspace_tab_manage,
    icon: Home01Icon,
};

/** The set of apps a sandbox can launch from its Manage tab, and each one's availability rule. */
export const LAUNCHABLE_APPS: AppDescriptor[] = [
    {
        kind: "terminal",
        title: m.workspace_tab_terminal,
        icon: TerminalIcon,
        command: "open_terminal_pane",
        available: () => true,
    },
    {
        kind: "agentweb",
        title: m.workspace_tab_agents,
        icon: BotIcon,
        command: "open_agent_web",
        available: (ctx) => ctx.installedAgents.length > 0,
    },
];
