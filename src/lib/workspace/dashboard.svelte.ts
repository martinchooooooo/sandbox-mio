import type { DashboardSectionId } from "./sections";

// Module-scoped so the choice survives the `{#key dashboardSandbox}` remount in
// WorkspaceContent.svelte; frontend memory only, gone on an app restart.

const sectionBySandbox = $state<Record<string, DashboardSectionId>>({});

/** The section last opened for `sandboxName`, `overview` when none. */
export function sectionFor(sandboxName: string): DashboardSectionId {
    return sectionBySandbox[sandboxName] ?? "overview";
}

export function setSection(sandboxName: string, id: DashboardSectionId): void {
    sectionBySandbox[sandboxName] = id;
}

export function forgetSection(sandboxName: string): void {
    delete sectionBySandbox[sandboxName];
}
