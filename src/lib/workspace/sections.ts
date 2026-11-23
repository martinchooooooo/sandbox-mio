import {
    Layout01Icon,
    Wrench01Icon,
    PlugSocketIcon,
    Shield01Icon,
    Activity01Icon,
    CloudServerIcon,
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
    | "remote";

export type SectionContext = { provider: ProviderId | undefined };

export type DashboardSection = {
    id: DashboardSectionId;
    icon: typeof Layout01Icon;
    label: () => string;
    visible(ctx: SectionContext): boolean;
};

/** The sections of a sandbox's Manage dashboard, in list order, and when each one is shown. */
export const DASHBOARD_SECTIONS: DashboardSection[] = [
    {
        id: "overview",
        icon: Layout01Icon,
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
];
