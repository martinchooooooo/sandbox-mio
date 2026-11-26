import { invoke } from "@tauri-apps/api/core";
import type { RegistryEntry } from "$lib/registry.svelte";

export function installPackage(name: string, pkg: string): Promise<void> {
    return invoke("install_package", { name, package: pkg });
}

export function uninstallPackage(name: string, pkg: string): Promise<void> {
    return invoke("uninstall_package", { name, package: pkg });
}

export function listPackages(workspacePath: string): Promise<string[]> {
    return invoke<string[]>("list_packages", { workspacePath });
}

export function listRegistry(name: string): Promise<RegistryEntry[]> {
    return invoke<RegistryEntry[]>("list_registry", { name });
}
