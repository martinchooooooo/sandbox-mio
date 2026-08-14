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
    import { PROVIDERS, providerLabel, type ProviderId } from "$lib/providers";
    import { onMount } from "svelte";

    let { projects, onCreated }: { projects: Project[]; onCreated: () => void } = $props();

    let mode = $state<"name" | "folder">("name");
    let name = $state("");
    let existingPath = $state<string | undefined>(undefined);
    let provider = $state<ProviderId>("podman");

    const action = createActionRunner();

    // Seeded from the persisted `defaultProvider` setting; the picker below overrides it per
    // sandbox without writing the choice back.
    onMount(async () => {
        try {
            const s = await invoke<{ defaultProvider?: ProviderId }>("get_settings");
            if (s.defaultProvider) provider = s.defaultProvider;
        } catch {
            /* keep the podman default */
        }
    });

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
                await invoke("create_sandbox", { name: trimmed, existingPath: path, provider });
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
            <Field.Field>
                <!-- TODO(i18n) -->
                <Field.Label>Runs on</Field.Label>
                <div data-provider-picker class="flex gap-2">
                    {#each PROVIDERS as option (option)}
                        <Button
                            data-provider-option={option}
                            aria-pressed={provider === option}
                            variant={provider === option ? "default" : "outline"}
                            size="sm"
                            disabled={action.busy !== null}
                            onclick={() => (provider = option)}
                        >{providerLabel(option)}</Button>
                    {/each}
                </div>
            </Field.Field>
        </Field.Group>
    </Field.Set>

    <StatusRow busy={action.busy} message={action.message} error={action.error} />
</li>
