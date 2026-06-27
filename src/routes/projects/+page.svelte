<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import * as Field from "$lib/components/ui/field";
    import { Input } from "$lib/components/ui/input";
    import { Switch } from "$lib/components/ui/switch";
    import { Channel, invoke } from "@tauri-apps/api/core";
    import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
    import { revealItemInDir } from "@tauri-apps/plugin-opener";
    import { onMount, onDestroy, tick } from "svelte";
    import { parseError } from "$lib/errors";
    import { m } from "$lib/paraglide/messages.js";

    interface Project {
        id: string;
        name: string;
        status: string;
        workspace_path: string;
    }

    interface TerminalSession {
        url: string;
        token: string;
    }

    type SyncStatus = "syncing" | "up-to-date" | "conflict" | "error";

    interface Deployment {
        name: string;
        host: string;
        status: SyncStatus;
    }

    let projectName = $state("");
    let projects = $state<Project[]>([]);
    let error = $state("");
    let events = $state<string[]>([]);
    let showLog = $state(false);
    let listening = $state(false);
    let logEl = $state<HTMLDivElement>();

    // Deployment state, overlaid onto the local sandbox list by name. A sandbox with an entry is
    // deployed to `deployments[name].host`; one without is local-only. Polled, since sync status
    // changes outside our event stream.
    let hosts = $state<string[]>([]);
    let deployments = $state<Record<string, Deployment>>({});
    // Which sandbox's host picker is open, and the host chosen in it.
    let deployingFor = $state<string | null>(null);
    let chosenHost = $state("");
    // The toggle is optimistic: Mutagen does not surface a "paused" status, so on/off is tracked
    // locally, defaulting on for a freshly listed deployment.
    let syncOn = $state<Record<string, boolean>>({});

    // The one-time login token from the last openTerminal call, shown in a banner per sandbox so the
    // user can paste it into the terminal window's login page (auto-login is a deferred follow-up).
    let terminalToken = $state<Record<string, string>>({});

    let packages = $state<Record<string, string[]>>({});
    let packageInput = $state<Record<string, string>>({});
    let packageBusy = $state<Record<string, boolean>>({});

    async function loadProjects() {
        try {
            projects = await invoke<Project[]>("list_sandboxes");
            for (const p of projects) loadPackages(p);
        } catch (e) {
            error = parseError(e).message;
        }
    }

    // Reads from the host manifest, so no polling needed — only install/uninstall change it.
    async function loadPackages(project: Project) {
        try {
            packages[project.name] = await invoke<string[]>("list_packages", {
                workspacePath: project.workspace_path,
            });
        } catch (e) {
            error = parseError(e).message;
        }
    }

    async function installPackage(project: Project) {
        const pkg = (packageInput[project.name] ?? "").trim();
        if (!pkg) return;
        packageBusy[project.name] = true;
        try {
            error = "";
            await invoke("install_package", { name: project.name, package: pkg });
            packageInput[project.name] = "";
            await loadPackages(project);
        } catch (e) {
            error = parseError(e).message;
        } finally {
            packageBusy[project.name] = false;
        }
    }

    async function uninstallPackage(project: Project, pkg: string) {
        packageBusy[project.name] = true;
        try {
            error = "";
            await invoke("uninstall_package", { name: project.name, package: pkg });
            await loadPackages(project);
        } catch (e) {
            error = parseError(e).message;
        } finally {
            packageBusy[project.name] = false;
        }
    }

    async function loadDeployments() {
        try {
            const list = await invoke<Deployment[]>("sandbox_deployments");
            deployments = Object.fromEntries(list.map((d) => [d.name, d]));
            for (const d of list) syncOn[d.name] ??= true;
        } catch (e) {
            error = parseError(e).message;
        }
    }

    async function loadHosts() {
        try {
            hosts = await invoke<string[]>("list_sandbox_hosts");
        } catch (e) {
            error = parseError(e).message;
        }
    }

    function openPicker(project: Project) {
        deployingFor = project.id;
        chosenHost = hosts[0] ?? "";
    }

    async function deploy(project: Project) {
        if (!chosenHost) return;
        try {
            error = "";
            await invoke("deploy_sandbox", { name: project.name, host: chosenHost });
            deployingFor = null;
            await loadDeployments();
        } catch (e) {
            error = parseError(e).message;
        }
    }

    async function undeploy(project: Project) {
        try {
            error = "";
            await invoke("undeploy_sandbox", { name: project.name });
            await Promise.all([loadDeployments(), loadProjects()]);
        } catch (e) {
            error = parseError(e).message;
        }
    }

    async function destroyProject(project: Project) {
        try {
            error = "";
            await invoke("destroy_sandbox", { name: project.name, force: false });
        } catch (e) {
            const err = parseError(e);
            // The guard refuses to discard unsynced remote work; confirm, then force.
            if (err.kind === "UnsyncedChanges") {
                if (!confirm(`${err.message}\n\nDestroy anyway and discard them?`)) return;
                try {
                    await invoke("destroy_sandbox", { name: project.name, force: true });
                } catch (e2) {
                    error = parseError(e2).message;
                    return;
                }
            } else {
                error = err.message;
                return;
            }
        }
        await Promise.all([loadDeployments(), loadProjects()]);
    }

    async function toggleSync(project: Project, on: boolean) {
        syncOn[project.name] = on;
        try {
            error = "";
            await invoke(on ? "enable_sync" : "disable_sync", { name: project.name });
            await loadDeployments();
        } catch (e) {
            syncOn[project.name] = !on; // revert the optimistic flip
            error = parseError(e).message;
        }
    }

    async function createProject() {
        const name = projectName.trim();
        if (!name) return;
        try {
            await invoke("create_sandbox", { name });
            projectName = "";
            error = "";
            await loadProjects();
        } catch (e) {
            error = parseError(e).message;
        }
    }

    async function start(project: Project) {
        try {
            await invoke("start_sandbox", { name: project.name });
        } catch (e) {
            error = parseError(e).message;
        }
    }

    async function stop(project: Project) {
        try {
            await invoke("stop_sandbox", { name: project.name });
        } catch (e) {
            error = parseError(e).message;
        }
    }

    // Opens the in-app web terminal: starts the zellij web server (via the command), then loads the
    // returned loopback URL in a dedicated top-level WebviewWindow (an iframe is blocked by the
    // terminal's X-Frame-Options: DENY). A deterministic per-sandbox label keeps one window per
    // sandbox — a second click focuses the existing window instead of stacking a duplicate.
    async function openTerminal(project: Project) {
        try {
            error = "";
            const session = await invoke<TerminalSession>("open_terminal", {
                name: project.name,
                workspacePath: project.workspace_path,
            });
            terminalToken[project.name] = session.token;
            const label = `terminal-${project.name}`;
            const existing = await WebviewWindow.getByLabel(label);
            if (existing) {
                await existing.setFocus();
            } else {
                new WebviewWindow(label, { url: session.url, title: `${project.name} — terminal` });
            }
        } catch (e) {
            error = parseError(e).message;
        }
    }

    async function copyToken(project: Project) {
        const token = terminalToken[project.name];
        if (token) await navigator.clipboard.writeText(token);
    }

    async function openWorkspace(project: Project) {
        try {
            await revealItemInDir(project.workspace_path);
        } catch (e) {
            error = parseError(e).message;
        }
    }

    const ACTION_STATUS: Record<string, string> = {
        start: "running",
        unpause: "running",
        stop: "exited",
        die: "exited",
        died: "exited",
        kill: "exited",
        pause: "paused",
        create: "created",
        destroy: "removed",
    };

    async function startListening() {
        if (listening) return;
        listening = true;
        const channel = new Channel<Record<string, unknown>>();
        channel.onmessage = async (ev) => {
            const type = String((ev.Type ?? ev.type) ?? "");
            const action = String((ev.Action ?? ev.action ?? ev.Status ?? ev.status) ?? "");
            const attrs = (ev.Actor as any)?.Attributes ?? (ev.actor as any)?.attributes ?? {};
            const name = String(attrs.name ?? (ev.Name as any) ?? (ev.name as any) ?? ev.id ?? "");
            const time = new Date().toLocaleTimeString();
            events.push(`[${time}] ${type} ${action}${name ? ` — ${name}` : ""}`);

            if (type === "container" && name && action in ACTION_STATUS) {
                const idx = projects.findIndex((p) => p.name === name);
                if (idx !== -1) projects[idx] = { ...projects[idx], status: ACTION_STATUS[action] };
            }

            await tick();
            logEl?.scrollTo({ top: logEl.scrollHeight, behavior: "smooth" });
        };
        try {
            await invoke("listen_events", { onEvent: channel });
        } catch (e) {
            events.push(`Error: ${parseError(e).message}`);
            listening = false;
        }
    }

    function toggleLog() {
        showLog = !showLog;
    }

    function translateStatus(status: string): string {
        const map: Record<string, () => string> = {
            running:     m.status_running,
            paused:      m.status_paused,
            exited:      m.status_exited,
            created:     m.status_created,
            removed:     m.status_removed,
            syncing:     m.status_syncing,
            "up-to-date": m.status_up_to_date,
            conflict:    m.status_conflict,
            error:       m.status_error,
        };
        return map[status]?.() ?? status;
    }

    // Colour for a deployed sandbox's sync-status dot, mirroring the container-state dot idiom.
    function syncColor(status: SyncStatus): string {
        return status === "up-to-date" ? "bg-green-500" :
               status === "syncing"    ? "bg-blue-400" :
               status === "conflict"   ? "bg-yellow-400" :
               "bg-red-500";
    }

    let pollId: ReturnType<typeof setInterval>;

    onMount(() => {
        loadProjects();
        loadHosts();
        loadDeployments();
        startListening();
        // Sync status drifts outside the container event stream, so poll the deployment overlay.
        pollId = setInterval(loadDeployments, 3000);
    });

    onDestroy(() => clearInterval(pollId));
</script>

<main class="p-6 pb-52">
    <Button href="/" variant="outline" class="mb-6">{m.back()}</Button>

    <h1 class="text-center text-3xl mb-8">{m.projects_title()}</h1>

    <div class="max-w-2xl mx-auto">
        <Field.Set class="w-full">
            <Field.Group>
                <Field.Field>
                    <Field.Label>{m.projects_name_label()}</Field.Label>
                    <div class="flex w-full gap-2">
                        <Input placeholder={m.projects_name_placeholder()} bind:value={projectName} />
                        <Button onclick={createProject}>{m.projects_create()}</Button>
                    </div>
                </Field.Field>
            </Field.Group>
        </Field.Set>

        {#if error}
            <p class="text-destructive mt-4 text-sm">{error}</p>
        {/if}

        {#if projects.length > 0}
            <ul class="mt-6 space-y-2">
                {#each projects as project (project.id)}
                    {@const deployment = deployments[project.name]}
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
                                <button onclick={() => openWorkspace(project)} title={project.workspace_path}>{m.projects_folder()}</button>
                                {#if deployment}
                                    <label class="flex items-center gap-1 text-xs">
                                        <Switch checked={syncOn[project.name] ?? true} onCheckedChange={(v) => toggleSync(project, v)} />
                                        {m.projects_sync()}
                                    </label>
                                    <button onclick={() => undeploy(project)}>{m.projects_undeploy()}</button>
                                {:else}
                                    {#if project.status === "running"}
                                        <button onclick={() => stop(project)}>{m.projects_stop()}</button>
                                    {:else}
                                        <button onclick={() => start(project)}>{m.projects_start()}</button>
                                    {/if}
                                    <button onclick={() => openTerminal(project)}>{m.projects_terminal()}</button>
                                    <button onclick={() => openPicker(project)}>{m.projects_deploy()}</button>
                                {/if}
                                <button onclick={() => destroyProject(project)} class="text-destructive">{m.projects_destroy()}</button>
                            </span>
                        </div>

                        {#if terminalToken[project.name]}
                            <div class="flex items-center gap-2 text-xs">
                                <span class="text-muted-foreground shrink-0">{m.projects_terminal_token()}</span>
                                <span class="font-mono truncate">{terminalToken[project.name]}</span>
                                <button class="shrink-0" onclick={() => copyToken(project)}>{m.projects_terminal_copy()}</button>
                            </div>
                        {/if}

                        {#if deployingFor === project.id}
                            <div class="flex items-center gap-2">
                                {#if hosts.length === 0}
                                    <span class="text-muted-foreground text-xs flex-1">{m.projects_no_hosts()}</span>
                                {:else}
                                    <select bind:value={chosenHost} class="flex-1 rounded border bg-background px-2 py-1 text-xs">
                                        {#each hosts as host}
                                            <option value={host}>{host}</option>
                                        {/each}
                                    </select>
                                    <Button size="sm" onclick={() => deploy(project)}>{m.projects_deploy_confirm()}</Button>
                                {/if}
                                <Button size="sm" variant="outline" onclick={() => (deployingFor = null)}>{m.projects_cancel()}</Button>
                            </div>
                        {/if}

                        <div class="flex flex-col gap-1.5 border-t pt-2">
                            <div class="flex items-center gap-2">
                                <span class="text-muted-foreground text-xs shrink-0">{m.projects_packages()}</span>
                                <input
                                    class="flex-1 rounded border bg-background px-2 py-1 text-xs disabled:opacity-50"
                                    placeholder={m.projects_package_placeholder()}
                                    bind:value={packageInput[project.name]}
                                    disabled={packageBusy[project.name]}
                                    onkeydown={(e) => e.key === "Enter" && installPackage(project)}
                                />
                                <Button size="sm" disabled={packageBusy[project.name]} onclick={() => installPackage(project)}>
                                    {m.projects_install()}
                                </Button>
                            </div>
                            {#if (packages[project.name] ?? []).length > 0}
                                <!-- <span> not <li>: nested <li> breaks WebView2's `li*=name` WebDriver lookup. -->
                                <div class="flex flex-wrap gap-1.5">
                                    {#each packages[project.name] as pkg}
                                        <span class="flex items-center gap-1 rounded bg-muted px-2 py-0.5 text-xs">
                                            <span class="font-mono">{pkg}</span>
                                            <button
                                                class="text-destructive disabled:opacity-50"
                                                disabled={packageBusy[project.name]}
                                                title={m.projects_uninstall()}
                                                onclick={() => uninstallPackage(project, pkg)}>✕</button>
                                        </span>
                                    {/each}
                                </div>
                            {:else}
                                <span class="text-muted-foreground text-xs">{m.projects_no_packages()}</span>
                            {/if}
                        </div>
                    </li>
                {/each}
            </ul>
        {/if}
    </div>
</main>

<div class="fixed bottom-0 left-0 right-0">
    {#if showLog}
        <div class="h-44 bg-background border-t flex flex-col">
            <div class="flex items-center justify-between px-4 py-1.5 border-b text-xs text-muted-foreground shrink-0">
                <span class="font-medium">
                    {m.event_log()}
                    <span class={listening ? "text-green-500" : "text-muted-foreground"}>
                        {listening ? m.event_log_live() : m.event_log_stopped()}
                    </span>
                </span>
                <button onclick={() => (showLog = false)} class="hover:text-foreground leading-none">✕</button>
            </div>
            <div bind:this={logEl} class="flex-1 overflow-y-auto px-4 py-2 font-mono text-xs space-y-0.5">
                {#if events.length === 0}
                    <p class="text-muted-foreground">{m.event_log_waiting()}</p>
                {:else}
                    {#each events as line}
                        <div>{line}</div>
                    {/each}
                {/if}
            </div>
        </div>
    {/if}

    <div class="flex justify-end px-4 py-2 bg-background border-t">
        <Button variant="outline" size="sm" onclick={toggleLog}>
            {showLog ? m.event_log_hide() : m.event_log()}
        </Button>
    </div>
</div>
