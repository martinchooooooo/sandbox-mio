<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import { Switch } from "$lib/components/ui/switch";
    import { invoke } from "@tauri-apps/api/core";
    import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
    import { revealItemInDir } from "@tauri-apps/plugin-opener";
    import { onMount } from "svelte";
    import { parseError } from "$lib/errors";
    import { m } from "$lib/paraglide/messages.js";
    import {
        translateStatus,
        syncColor,
        type Project,
        type Deployment,
        type TerminalSession,
    } from "$lib/sandbox";

    let {
        project,
        deployment,
        hosts,
        onError,
        onChanged,
    }: {
        project: Project;
        // Deployment overlay for this sandbox by name; absent means local-only.
        deployment: Deployment | undefined;
        hosts: string[];
        onError: (message: string) => void;
        // Tells the parent to reload its sandbox and deployment lists after a mutating action.
        onChanged: () => void;
    } = $props();

    let packages = $state<string[]>([]);
    let packageInput = $state("");
    let packageBusy = $state(false);

    // The one-time login token from the last openTerminal call, shown in a banner so the user can
    // paste it into the terminal window's login page (auto-login is a deferred follow-up).
    let terminalToken = $state("");

    // Whether this sandbox's host picker is open, and the host chosen within it.
    let picking = $state(false);
    let chosenHost = $state("");

    // The toggle is optimistic: Mutagen does not surface a "paused" status, so on/off is tracked
    // locally, defaulting on for a freshly listed deployment.
    let syncOn = $state(true);

    // Reads from the host manifest, so no polling needed — only install/uninstall change it.
    async function loadPackages() {
        try {
            packages = await invoke<string[]>("list_packages", {
                workspacePath: project.workspace_path,
            });
        } catch (e) {
            onError(parseError(e).message);
        }
    }

    async function installPackage() {
        const pkg = packageInput.trim();
        if (!pkg) return;
        packageBusy = true;
        try {
            onError("");
            await invoke("install_package", { name: project.name, package: pkg });
            packageInput = "";
            await loadPackages();
        } catch (e) {
            onError(parseError(e).message);
        } finally {
            packageBusy = false;
        }
    }

    async function uninstallPackage(pkg: string) {
        packageBusy = true;
        try {
            onError("");
            await invoke("uninstall_package", { name: project.name, package: pkg });
            await loadPackages();
        } catch (e) {
            onError(parseError(e).message);
        } finally {
            packageBusy = false;
        }
    }

    function openPicker() {
        picking = true;
        chosenHost = hosts[0] ?? "";
    }

    async function deploy() {
        if (!chosenHost) return;
        try {
            onError("");
            await invoke("deploy_sandbox", { name: project.name, host: chosenHost });
            picking = false;
            onChanged();
        } catch (e) {
            onError(parseError(e).message);
        }
    }

    async function undeploy() {
        try {
            onError("");
            await invoke("undeploy_sandbox", { name: project.name });
            onChanged();
        } catch (e) {
            onError(parseError(e).message);
        }
    }

    async function destroyProject() {
        try {
            onError("");
            await invoke("destroy_sandbox", { name: project.name, force: false });
        } catch (e) {
            const err = parseError(e);
            // The guard refuses to discard unsynced remote work; confirm, then force.
            if (err.kind === "UnsyncedChanges") {
                if (!confirm(`${err.message}\n\nDestroy anyway and discard them?`)) return;
                try {
                    await invoke("destroy_sandbox", { name: project.name, force: true });
                } catch (e2) {
                    onError(parseError(e2).message);
                    return;
                }
            } else {
                onError(err.message);
                return;
            }
        }
        onChanged();
    }

    async function toggleSync(on: boolean) {
        syncOn = on;
        try {
            onError("");
            await invoke(on ? "enable_sync" : "disable_sync", { name: project.name });
            onChanged();
        } catch (e) {
            syncOn = !on; // revert the optimistic flip
            onError(parseError(e).message);
        }
    }

    async function start() {
        try {
            await invoke("start_sandbox", { name: project.name });
        } catch (e) {
            onError(parseError(e).message);
        }
    }

    async function stop() {
        try {
            await invoke("stop_sandbox", { name: project.name });
        } catch (e) {
            onError(parseError(e).message);
        }
    }

    // Opens the in-app web terminal: starts the zellij web server (via the command), then loads the
    // returned loopback URL in a dedicated top-level WebviewWindow (an iframe is blocked by the
    // terminal's X-Frame-Options: DENY). A deterministic per-sandbox label keeps one window per
    // sandbox — a second click focuses the existing window instead of stacking a duplicate.
    async function openTerminal() {
        try {
            onError("");
            const session = await invoke<TerminalSession>("open_terminal", {
                name: project.name,
                workspacePath: project.workspace_path,
            });
            terminalToken = session.token;
            const label = `terminal-${project.name}`;
            const existing = await WebviewWindow.getByLabel(label);
            if (existing) {
                await existing.setFocus();
            } else {
                new WebviewWindow(label, { url: session.url, title: `${project.name} — terminal` });
            }
        } catch (e) {
            onError(parseError(e).message);
        }
    }

    async function copyToken() {
        if (terminalToken) await navigator.clipboard.writeText(terminalToken);
    }

    async function openWorkspace() {
        try {
            await revealItemInDir(project.workspace_path);
        } catch (e) {
            onError(parseError(e).message);
        }
    }

    onMount(loadPackages);
</script>

<li class="flex flex-col gap-2 rounded-lg border p-3 text-sm">
    <div class="flex justify-between items-center gap-2">
        <span class="flex items-center gap-2 font-medium min-w-0">
            <span class={[
                "inline-block h-2 w-2 rounded-full shrink-0",
                deployment ? syncColor(deployment.status) :
                project.status === "running" ? "bg-green-500" :
                project.status === "paused"  ? "bg-yellow-400" :
                project.status === "exited"  ? "bg-red-400" :
                "bg-muted-foreground/40"
            ].join(" ")}></span>
            <span class="truncate">{project.name}</span>
            {#if deployment}
                <span class="text-muted-foreground text-xs shrink-0">→ {deployment.host}</span>
            {/if}
        </span>
        <span class="flex items-center gap-2 shrink-0">
            <span class="text-muted-foreground text-xs">{translateStatus(deployment ? deployment.status : project.status)}</span>
            <button onclick={openWorkspace} title={project.workspace_path}>{m.projects_folder()}</button>
            {#if deployment}
                <label class="flex items-center gap-1 text-xs">
                    <Switch checked={syncOn} onCheckedChange={toggleSync} />
                    {m.projects_sync()}
                </label>
                <button onclick={undeploy}>{m.projects_undeploy()}</button>
            {:else}
                {#if project.status === "running"}
                    <button onclick={stop}>{m.projects_stop()}</button>
                {:else}
                    <button onclick={start}>{m.projects_start()}</button>
                {/if}
                <button onclick={openTerminal}>{m.projects_terminal()}</button>
                <button onclick={openPicker}>{m.projects_deploy()}</button>
            {/if}
            <button onclick={destroyProject} class="text-destructive">{m.projects_destroy()}</button>
        </span>
    </div>

    {#if terminalToken}
        <div class="flex items-center gap-2 text-xs">
            <span class="text-muted-foreground shrink-0">{m.projects_terminal_token()}</span>
            <span class="font-mono truncate">{terminalToken}</span>
            <button class="shrink-0" onclick={copyToken}>{m.projects_terminal_copy()}</button>
        </div>
    {/if}

    {#if picking}
        <div class="flex items-center gap-2">
            {#if hosts.length === 0}
                <span class="text-muted-foreground text-xs flex-1">{m.projects_no_hosts()}</span>
            {:else}
                <select bind:value={chosenHost} class="flex-1 rounded border bg-background px-2 py-1 text-xs">
                    {#each hosts as host}
                        <option value={host}>{host}</option>
                    {/each}
                </select>
                <Button size="sm" onclick={deploy}>{m.projects_deploy_confirm()}</Button>
            {/if}
            <Button size="sm" variant="outline" onclick={() => (picking = false)}>{m.projects_cancel()}</Button>
        </div>
    {/if}

    <div class="flex flex-col gap-1.5 border-t pt-2">
        <div class="flex items-center gap-2">
            <span class="text-muted-foreground text-xs shrink-0">{m.projects_packages()}</span>
            <input
                class="flex-1 rounded border bg-background px-2 py-1 text-xs disabled:opacity-50"
                placeholder={m.projects_package_placeholder()}
                bind:value={packageInput}
                disabled={packageBusy}
                onkeydown={(e) => e.key === "Enter" && installPackage()}
            />
            <Button size="sm" disabled={packageBusy} onclick={installPackage}>
                {m.projects_install()}
            </Button>
        </div>
        {#if packages.length > 0}
            <!-- <span> not <li>: nested <li> breaks WebView2's `li*=name` WebDriver lookup. -->
            <div class="flex flex-wrap gap-1.5">
                {#each packages as pkg}
                    <span class="flex items-center gap-1 rounded bg-muted px-2 py-0.5 text-xs">
                        <span class="font-mono">{pkg}</span>
                        <button
                            class="text-destructive disabled:opacity-50"
                            disabled={packageBusy}
                            title={m.projects_uninstall()}
                            onclick={() => uninstallPackage(pkg)}>✕</button>
                    </span>
                {/each}
            </div>
        {:else}
            <span class="text-muted-foreground text-xs">{m.projects_no_packages()}</span>
        {/if}
    </div>
</li>
