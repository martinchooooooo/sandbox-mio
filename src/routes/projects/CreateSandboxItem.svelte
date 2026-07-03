<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import * as Field from "$lib/components/ui/field";
    import { Input } from "$lib/components/ui/input";
    import { invoke } from "@tauri-apps/api/core";
    import { open } from "@tauri-apps/plugin-dialog";
    import { m } from "$lib/paraglide/messages.js";
    import { createActionRunner } from "$lib/actions.svelte";
    import StatusRow from "./StatusRow.svelte";
    import type { Project } from "$lib/sandbox";

    let { projects, onCreated }: { projects: Project[]; onCreated: () => void } = $props();

    let mode = $state<"name" | "folder">("name");
    let name = $state("");
    let existingPath = $state<string | undefined>(undefined);

    const action = createActionRunner();

    // Path may come from either OS separator (the native dialog returns the host's convention).
    function basename(path: string): string {
        return path.split(/[/\\]/).filter(Boolean).pop() ?? path;
    }

    async function chooseFolder() {
        const picked = await open({ directory: true });
        if (!picked) return;
        existingPath = picked;
        name = basename(picked);
    }

    function backToName() {
        mode = "name";
        existingPath = undefined;
    }

    async function createProject() {
        const trimmed = name.trim();
        if (!trimmed) return;
        const path = existingPath;
        const ok = await action.run(
            "create",
            async () => {
                if (projects.some((p) => p.name === trimmed)) {
                    // TODO(i18n)
                    throw { kind: "Backend", message: `A sandbox named "${trimmed}" already exists.` };
                }
                await invoke("create_sandbox", { name: trimmed, existingPath: path });
            },
            { message: m.projects_status_creating({ name: trimmed }) }
        );
        if (ok) {
            name = "";
            mode = "name";
            existingPath = undefined;
            onCreated();
        }
    }
</script>

<li data-create-row class="flex flex-col gap-2 rounded-lg border p-3 text-sm">
    <Field.Set class="w-full">
        <Field.Group>
            <Field.Field>
                <Field.Label>{m.projects_name_label()}</Field.Label>
                {#if mode === "name"}
                    <div class="flex w-full gap-2">
                        <Input placeholder={m.projects_name_placeholder()} bind:value={name} />
                        <Button
                            loading={action.busy === "create"}
                            disabled={action.busy !== null}
                            onclick={createProject}
                        >{m.projects_create()}</Button>
                    </div>
                    <Button variant="link" size="sm" class="self-start px-0" onclick={() => (mode = "folder")}>
                        <!-- TODO(i18n) -->
                        or use an existing folder…
                    </Button>
                {:else}
                    {#if existingPath === undefined}
                        <div class="flex w-full items-center gap-2">
                            <Button variant="outline" onclick={chooseFolder}>
                                <!-- TODO(i18n) -->
                                Choose folder…
                            </Button>
                            <Button variant="ghost" size="sm" aria-label="Back" onclick={backToName}>x</Button>
                        </div>
                    {:else}
                        <p class="text-muted-foreground truncate text-xs" title={existingPath}>{existingPath}</p>
                        <div class="flex w-full gap-2">
                            <Input placeholder={m.projects_name_placeholder()} bind:value={name} />
                            <Button
                                loading={action.busy === "create"}
                                disabled={action.busy !== null}
                                onclick={createProject}
                            >{m.projects_create()}</Button>
                            <Button variant="ghost" size="sm" aria-label="Back" onclick={backToName}>x</Button>
                        </div>
                    {/if}
                {/if}
            </Field.Field>
        </Field.Group>
    </Field.Set>

    <StatusRow busy={action.busy} message={action.message} error={action.error} />
</li>
