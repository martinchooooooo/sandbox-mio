import { DashboardSquare01Icon, Home01Icon, TerminalIcon, BotIcon, AiChat02Icon } from "@hugeicons/core-free-icons";
import { m } from "$lib/paraglide/messages.js";
import type { AgentDescriptor } from "$lib/agents/types";
import { openAgentChat, openAgentWeb, openTerminalPane } from "$lib/api/panes";
import type { AppKind } from "./state.svelte";

export type LaunchContext = { installedAgents: AgentDescriptor[] };

export type AppDescriptor = {
    kind: AppKind;
    title: () => string;
    icon: typeof Home01Icon;
    /** Opens the app's pane for a sandbox, activating it. */
    open: (sandbox: string, workspacePath: string) => Promise<void>;
    /** One plain-language line for the dashboard card. */
    description: () => string;
    /** Shown on the disabled card when `available` is false: what the user has to do. */
    unavailableHint: () => string;
    available(ctx: LaunchContext): boolean;
};

/** Dashboard entry metadata. Not a catalogue entry: always present, not launchable, not closable. */
export const HOME_APP = {
    title: m.workspace_tab_manage,
    icon: DashboardSquare01Icon,
};

/** The set of apps a sandbox can launch from its dashboard, and each one's availability rule. */
export const LAUNCHABLE_APPS: AppDescriptor[] = [
    {
        kind: "agentchat",
        title: m.workspace_tab_chat,
        icon: AiChat02Icon,
        open: openAgentChat,
        description: m.app_chat_desc,
        unavailableHint: m.app_chat_unavailable,
        // The native chat drives pi only (docs/features/native-agent-ui/02_dev.md).
        available: (ctx) => ctx.installedAgents.some((a) => a.id === "pi"),
    },
    {
        kind: "agentweb",
        title: m.workspace_tab_agents,
        icon: BotIcon,
        open: openAgentWeb,
        description: m.app_agents_desc,
        unavailableHint: m.app_agents_unavailable,
        available: (ctx) => ctx.installedAgents.length > 0,
    },
    {
        kind: "terminal",
        title: m.workspace_tab_terminal,
        icon: TerminalIcon,
        open: openTerminalPane,
        description: m.app_terminal_desc,
        unavailableHint: () => "",
        available: () => true,
    },
];
