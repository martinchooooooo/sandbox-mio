import { invoke } from "@tauri-apps/api/core";
import type { ProxyStatus } from "$lib/proxy/status.svelte";

export function proxyStatus(name: string): Promise<ProxyStatus> {
    return invoke<ProxyStatus>("proxy_status", { name });
}

export function restartProxy(name: string): Promise<void> {
    return invoke("restart_proxy", { name });
}

export function watchProxyFlows(name: string): Promise<void> {
    return invoke("watch_proxy_flows", { name });
}

export function unwatchProxyFlows(name: string): Promise<void> {
    return invoke("unwatch_proxy_flows", { name });
}

/** How many stored secrets a deploy of the sandbox would push to the remote host. */
export function deploySecretCount(name: string): Promise<number> {
    return invoke<number>("deploy_secret_count", { name });
}
