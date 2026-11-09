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
        label: () => "Overview", // TODO(i18n)
        visible: () => true,
    },
    {
        id: "tools",
        icon: Wrench01Icon,
        label: () => "Tools", // TODO(i18n)
        visible: () => true,
    },
    {
        id: "ports",
        icon: PlugSocketIcon,
        label: () => "Ports", // TODO(i18n)
        visible: () => true,
    },
    {
        id: "network",
        icon: Shield01Icon,
        label: () => "Network", // TODO(i18n)
        visible: () => true,
    },
    {
        id: "traffic",
        icon: Activity01Icon,
        label: () => "Traffic", // TODO(i18n)
        visible: () => true,
    },
    {
        id: "remote",
        icon: CloudServerIcon,
        label: () => "Remote", // TODO(i18n)
        visible: ({ provider }) => (provider ? (capabilitiesFor(provider)?.remoteTargets ?? false) : false),
    },
    {
        id: "danger",
        icon: Alert02Icon,
        label: () => "Danger zone", // TODO(i18n)
        separated: true,
        visible: () => true,
    },
];
