import { invoke } from "@tauri-apps/api/core";
import type { Deployment } from "$lib/sandbox";

export function deploySandbox(name: string, host: string): Promise<void> {
    return invoke("deploy_sandbox", { name, host });
}

export function undeploySandbox(name: string): Promise<void> {
    return invoke("undeploy_sandbox", { name });
}

export function sandboxDeployments(): Promise<Deployment[]> {
    return invoke<Deployment[]>("sandbox_deployments");
}

export function enableSync(name: string): Promise<void> {
    return invoke("enable_sync", { name });
}

export function disableSync(name: string): Promise<void> {
    return invoke("disable_sync", { name });
}
