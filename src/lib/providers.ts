import { invoke } from "@tauri-apps/api/core";
import { m } from "$lib/paraglide/messages.js";

/** Which engine runs a sandbox (mirrors Rust `provider::types::ProviderId`). */
export type ProviderId = "podman" | "microsandbox";

/** Every selectable provider, in the order the create-sandbox picker offers them. */
export const PROVIDERS: ProviderId[] = ["podman", "microsandbox"];

/** Feature differences between engines (mirrors Rust `provider::types::ProviderCapabilities`). */
export interface ProviderCapabilities {
    remoteTargets: boolean;
    eventStream: boolean;
    hostBindMount: boolean;
    /** Whether the engine enforces `.mio/net-policy.json` itself (microsandbox). */
    egressPolicy: boolean;
    /** Whether the proxy sidecar enforces `.mio/net-policy.json` when the sandbox has one (Podman). */
    sidecarEgress: boolean;
}

const LABELS: Record<ProviderId, () => string> = {
    podman: m.provider_podman_label,
    microsandbox: m.provider_microsandbox_label,
};

export function providerLabel(provider: ProviderId): string {
    return LABELS[provider]();
}

export function providerCapabilities(provider: ProviderId): Promise<ProviderCapabilities> {
    return invoke<ProviderCapabilities>("provider_capabilities", { provider });
}

/** Capabilities for every provider at once, for a view that renders sandboxes of both. */
export async function allCapabilities(): Promise<Record<ProviderId, ProviderCapabilities>> {
    const entries = await Promise.all(
        PROVIDERS.map(async (p) => [p, await providerCapabilities(p)] as const),
    );
    return Object.fromEntries(entries) as Record<ProviderId, ProviderCapabilities>;
}
