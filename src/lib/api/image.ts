import { invoke } from "@tauri-apps/api/core";
import type { ProviderCapabilities, ProviderId } from "$lib/providers";

/** The base image version this build ships. */
export function baseImageVersion(): Promise<number> {
    return invoke<number>("base_image_version");
}

export function buildBaseImage(): Promise<void> {
    return invoke("build_base_image");
}

export function baseImageReady(): Promise<boolean> {
    return invoke<boolean>("base_image_ready");
}

export function providerCapabilities(provider: ProviderId): Promise<ProviderCapabilities> {
    return invoke<ProviderCapabilities>("provider_capabilities", { provider });
}

/** Null when the user never dismissed an update for the workspace. */
export function getDismissedImageVersion(workspacePath: string): Promise<number | null> {
    return invoke<number | null>("get_dismissed_image_version", { workspacePath });
}

export function dismissImageUpdate(workspacePath: string, version: number): Promise<void> {
    return invoke("dismiss_image_update", { workspacePath, version });
}
