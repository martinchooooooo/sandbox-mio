import { invoke } from "@tauri-apps/api/core";

export function listSandboxHosts(): Promise<string[]> {
    return invoke<string[]>("list_sandbox_hosts");
}
