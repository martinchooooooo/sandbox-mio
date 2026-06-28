<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import { invoke } from "@tauri-apps/api/core";
    import { onMount } from "svelte";
    import { m } from "$lib/paraglide/messages.js";
    import { registryEntries, ensureRegistry } from "$lib/registry.svelte";
    import { portal } from "$lib/portal";
    import type { createActionRunner } from "$lib/actions.svelte";

    let {
        sandboxName,
        workspacePath,
        action,
    }: {
        sandboxName: string;
        workspacePath: string;
        action: ReturnType<typeof createActionRunner>;
    } = $props();

    let packages = $state<string[]>([]);
    let packageInput = $state("");
    let showSuggestions = $state(false);
    let packageInputEl = $state<HTMLInputElement>();
    let dropdownStyle = $state("");

    const filteredEntries = $derived(
        packageInput.length === 0
            ? []
            : registryEntries().filter(e => e.short.includes(packageInput)).slice(0, 20)
    );

    function openSuggestions() {
        if (packageInputEl) {
            const r = packageInputEl.getBoundingClientRect();
            dropdownStyle = `top:${r.bottom + 4}px;left:${r.left}px;width:${r.width}px`;
        }
        showSuggestions = true;
        ensureRegistry(sandboxName);
    }

    async function loadPackages() {
        await action.run("load", async () => {
            packages = await invoke<string[]>("list_packages", { workspacePath });
        });
    }

    async function installPackage() {
        const pkg = packageInput.trim();
        if (!pkg) return;
        if (await action.run(
            "install",
            () => invoke("install_package", { name: sandboxName, package: pkg }),
            { message: m.projects_status_installing({ name: pkg }) }
        )) {
            packageInput = "";
            showSuggestions = false;
            await loadPackages();
        }
    }

    async function uninstallPackage(pkg: string) {
        if (await action.run(
            "uninstall",
            () => invoke("uninstall_package", { name: sandboxName, package: pkg }),
            { message: m.projects_status_uninstalling({ name: pkg }) }
        )) {
            await loadPackages();
        }
    }

    onMount(loadPackages);
</script>

<div class="flex flex-col gap-1.5 border-t pt-2">
    <div class="flex items-center gap-2">
        <span class="text-muted-foreground text-xs shrink-0">{m.projects_packages()}</span>
        <div class="flex-1">
            <input
                class="w-full rounded border bg-background px-2 py-1 text-xs disabled:opacity-50"
                placeholder={m.projects_package_placeholder()}
                bind:value={packageInput}
                bind:this={packageInputEl}
                disabled={action.busy !== null}
                onfocus={openSuggestions}
                onblur={() => setTimeout(() => (showSuggestions = false), 0)}
                onkeydown={(e) => e.key === "Enter" && installPackage()}
            />
            {#if showSuggestions && filteredEntries.length > 0}
                <div use:portal style={dropdownStyle} class="fixed z-50 max-h-72 overflow-y-auto rounded border bg-popover shadow-md">
                    {#each filteredEntries as entry (entry.short)}
                        <button
                            type="button"
                            class="flex w-full flex-col items-start px-2 py-1.5 text-xs hover:bg-muted"
                            onmousedown={(e) => { e.preventDefault(); packageInput = entry.short; showSuggestions = false; }}
                        >
                            <span class="font-mono">{entry.short}</span>
                            {#if entry.description}
                                <span class="text-muted-foreground">{entry.description}</span>
                            {/if}
                        </button>
                    {/each}
                </div>
            {/if}
        </div>
        <Button size="sm" loading={action.busy === "install"} disabled={action.busy !== null} onclick={installPackage}>
            {m.projects_install()}
        </Button>
    </div>
    {#if packages.length > 0}
        <!-- <span> not <li>: nested <li> breaks WebView2's `li*=name` WebDriver lookup. -->
        <div class="flex flex-wrap gap-1.5">
            {#each packages as pkg}
                <span class="flex items-center gap-1 rounded bg-muted px-2 py-0.5 text-xs">
                    <span class="font-mono">{pkg}</span>
                    <Button variant="ghost" size="icon-xs"
                        loading={action.busy === "uninstall"}
                        disabled={action.busy !== null}
                        title={m.projects_uninstall()}
                        onclick={() => uninstallPackage(pkg)}>✕</Button>
                </span>
            {/each}
        </div>
    {:else}
        <span class="text-muted-foreground text-xs">{m.projects_no_packages()}</span>
    {/if}
</div>
