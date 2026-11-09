import {
    DashboardSquare01Icon,
    Wrench01Icon,
    PlugSocketIcon,
    Shield01Icon,
    Activity01Icon,
    CloudServerIcon,
    Alert02Icon,
} from "@hugeicons/core-free-icons";
import type { ProviderId } from "$lib/providers";
import { capabilitiesFor } from "$lib/sandbox/state.svelte";
import { m } from "$lib/paraglide/messages.js";

export type DashboardSectionId =
    | "overview"
    | "tools"
    | "ports"
    | "network"
    | "traffic"
    | "remote"
    | "danger";

export type SectionContext = { provider: ProviderId | undefined };

export type DashboardSection = {
    id: DashboardSectionId;
    icon: typeof DashboardSquare01Icon;
    label: () => string;
    /** Renders a separator above the list entry. */
    separated?: boolean;
    visible(ctx: SectionContext): boolean;
};

/** The sections of a sandbox's Manage dashboard, in list order, and when each one is shown. */
export const DASHBOARD_SECTIONS: DashboardSection[] = [
    {
        id: "overview",
        icon: DashboardSquare01Icon,
        label: () => m.dashboard_section_overview(),
        visible: () => true,
    },
    {
        id: "tools",
        icon: Wrench01Icon,
        label: () => m.dashboard_section_tools(),
        visible: () => true,
    },
    {
        id: "ports",
        icon: PlugSocketIcon,
        label: () => m.dashboard_section_ports(),
        visible: () => true,
    },
    {
        id: "network",
        icon: Shield01Icon,
        label: () => m.dashboard_section_network(),
        visible: () => true,
    },
    {
        id: "traffic",
        icon: Activity01Icon,
        label: () => m.traffic_title(),
        visible: () => true,
    },
    {
        id: "remote",
        icon: CloudServerIcon,
        label: () => m.dashboard_section_remote(),
        visible: ({ provider }) => (provider ? (capabilitiesFor(provider)?.remoteTargets ?? false) : false),
    },
    {
        id: "danger",
        icon: Alert02Icon,
        label: () => m.dashboard_section_danger(),
        separated: true,
        visible: () => true,
    },
];
