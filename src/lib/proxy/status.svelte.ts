import { invoke } from "@tauri-apps/api/core";
import { parseError } from "$lib/errors";

// Module-scoped per-sandbox cache: each `proxy_status` call lists every instance of the provider,
// so the Overview summary, Network and Traffic sections share one answer.

/** Mirrors Rust `proxy::types::ProxyStatus` (camelCase per its own `serde(rename_all)`). */
export interface ProxyStatus {
    exists: boolean;
    running: boolean;
    proxyIp: string;
    network: string;
    provider: "podman" | "microsandbox";
    /** The microsandbox sidecar's SOCKS5 port; null for Podman. */
    hostPort: number | null;
    /** Whether a running microsandbox sidecar answers; null for Podman or a stopped sidecar. */
    healthy: boolean | null;
}

type Entry = { value: ProxyStatus | null; error: string | null; fetchedAt: number; loading: boolean };

/** A cached answer younger than this is reused (docs/features/manage-dashboard-redesign/02_dev.md, Resolved Decision 12). */
export const PROXY_STATUS_MAX_AGE_MS = 5000;

const entries = $state<Record<string, Entry>>({});
const inFlight = new Map<string, Promise<void>>();

const EMPTY: Entry = { value: null, error: null, fetchedAt: 0, loading: false };

export function proxyStatusFor(sandboxName: string): Entry {
    return entries[sandboxName] ?? EMPTY;
}

/** Always fetches `sandboxName`'s status; concurrent callers share one call. */
export function refreshProxyStatus(sandboxName: string): Promise<void> {
    const pending = inFlight.get(sandboxName);
    if (pending) return pending;
    const previous = entries[sandboxName] ?? EMPTY;
    entries[sandboxName] = { ...previous, loading: true };
    const promise = (async () => {
        try {
            const value = await invoke<ProxyStatus>("proxy_status", { name: sandboxName });
            entries[sandboxName] = { value, error: null, fetchedAt: Date.now(), loading: false };
        } catch (e) {
            // A failed call keeps the last value; the error alone marks the answer unknown.
            entries[sandboxName] = {
                value: previous.value,
                error: parseError(e).message,
                fetchedAt: 0,
                loading: false,
            };
        }
    })().finally(() => inFlight.delete(sandboxName));
    inFlight.set(sandboxName, promise);
    return promise;
}

/** Fetches unless a value younger than `PROXY_STATUS_MAX_AGE_MS` is cached. */
export function ensureProxyStatus(sandboxName: string): Promise<void> {
    const entry = entries[sandboxName];
    if (entry && entry.value && Date.now() - entry.fetchedAt < PROXY_STATUS_MAX_AGE_MS) {
        return Promise.resolve();
    }
    return refreshProxyStatus(sandboxName);
}

export function forgetProxyStatus(sandboxName: string): void {
    delete entries[sandboxName];
}
