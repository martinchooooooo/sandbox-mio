<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import * as Field from "$lib/components/ui/field";
    import { Input } from "$lib/components/ui/input";
    import { Channel, invoke } from "@tauri-apps/api/core";
    import { revealItemInDir } from "@tauri-apps/plugin-opener";
    import { onMount, tick } from "svelte";

    interface Project {
        id: string;
        name: string;
        status: string;
        workspace_path: string;
    }

    let projectName = $state("");
    let projects = $state<Project[]>([]);
    let error = $state("");
    let events = $state<string[]>([]);
    let showLog = $state(false);
    let listening = $state(false);
    let logEl = $state<HTMLDivElement>();

    async function loadProjects() {
        try {
            projects = await invoke<Project[]>("list_sandboxes");
        } catch (e) {
            error = String(e);
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
            error = String(e);
        }
    }

    async function start(project: Project) {
        try {
            await invoke("start_sandbox", { name: project.name });
        } catch (e) {
            error = String(e);
        }
    }

    async function stop(project: Project) {
        try {
            await invoke("stop_sandbox", { name: project.name });
        } catch (e) {
            error = String(e);
        }
    }

    async function openWorkspace(project: Project) {
        try {
            await revealItemInDir(project.workspace_path);
        } catch (e) {
            error = String(e);
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
            events.push(`Error: ${String(e)}`);
            listening = false;
        }
    }

    function toggleLog() {
        showLog = !showLog;
    }

    onMount(() => {
        loadProjects();
        startListening();
    });
</script>

<main class="p-6 pb-52">
    <Button href="/" variant="outline" class="mb-6">← Back</Button>

    <h1 class="text-center text-3xl mb-8">Sandpits</h1>

    <div class="max-w-sm mx-auto">
        <Field.Set class="w-full">
            <Field.Group>
                <Field.Field>
                    <Field.Label>Project name</Field.Label>
                    <div class="flex w-full gap-2">
                        <Input placeholder="my-project" bind:value={projectName} />
                        <Button onclick={createProject}>Create</Button>
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
                    <li class="flex justify-between items-center rounded-lg border p-3 text-sm">
                        <span class="flex items-center gap-2 font-medium">
                            <span class={[
                                "inline-block h-2 w-2 rounded-full shrink-0",
                                project.status === "running" ? "bg-green-500" :
                                project.status === "paused"  ? "bg-yellow-400" :
                                project.status === "exited"  ? "bg-red-400" :
                                "bg-muted-foreground/40"
                            ].join(" ")}></span>
                            {project.name}
                        </span>
                        <span class="text-muted-foreground text-xs">{project.status}</span>
                        <button onclick={() => openWorkspace(project)} title={project.workspace_path}>folder</button>
                        {#if project.status === "running"}
                            <button onclick={() => stop(project)}>stop</button>
                        {:else}
                            <button onclick={() => start(project)}>start</button>
                        {/if}
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
                    Event Log
                    <span class={listening ? "text-green-500" : "text-muted-foreground"}>
                        {listening ? "● live" : "○ stopped"}
                    </span>
                </span>
                <button onclick={() => (showLog = false)} class="hover:text-foreground leading-none">✕</button>
            </div>
            <div bind:this={logEl} class="flex-1 overflow-y-auto px-4 py-2 font-mono text-xs space-y-0.5">
                {#if events.length === 0}
                    <p class="text-muted-foreground">Waiting for events…</p>
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
            {showLog ? "Hide" : "Event Log"}
        </Button>
    </div>
</div>
