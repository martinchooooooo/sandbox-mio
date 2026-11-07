<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import * as Field from "$lib/components/ui/field";
    import { Input } from "$lib/components/ui/input";
    import AdvancedSection from "$lib/components/AdvancedSection.svelte";
    import { Switch } from "$lib/components/ui/switch";
    import { invoke } from "@tauri-apps/api/core";
    import { open } from "@tauri-apps/plugin-dialog";
    import { m } from "$lib/paraglide/messages.js";
    import { createActionRunner } from "$lib/actions.svelte";
    import StatusRow from "./StatusRow.svelte";
    import { PROVIDERS, providerCapabilities, providerLabel, type ProviderId } from "$lib/providers";
    import { allAgentApiHosts } from "$lib/agents/catalogue";
    import { onMount } from "svelte";
    import { checkPodman, podmanUnavailable } from "$lib/podman/setup.svelte";

    let { onCreated }: { onCreated: () => void } = $props();

    let mode = $state<"name" | "folder">("name");
    let name = $state("");
    let existingPath = $state<string | undefined>(undefined);
    let provider = $state<ProviderId>("podman");
    // Off by default (docs/features/network-proxy/02_dev.md, Resolved Decision 3).
    let proxy = $state(false);
    // Open by default (docs/features/microsandbox-network-policy/02_dev.md, Resolved Decision 3).
    let egress = $state<"open" | "allowlist">("open");
    let egressSupported = $state(false);

    $effect(() => {
        const p = provider;
        providerCapabilities(p)
            .then((caps) => {
                if (p === provider) egressSupported = caps.egressPolicy;
            })
            .catch(() => (egressSupported = false));
    });

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
        // Re-verify readiness right here rather than trusting the page's mount-time check: Podman
        // can be stopped (by this app, Podman Desktop, or a host restart) any time after the page
        // loaded, and both providers' base-image build shells out to it. On failure this flips the
        // shared `podmanUnavailable` state, which unmounts this component in favor of the parent's
        // "Start Podman" screen, so no separate error handling is needed here.
        await checkPodman();
        if (podmanUnavailable()) return;
        const path = existingPath;
        const ok = await action.run(
            "create",
            async () => {
                const network = egressSupported
                    ? { egress, starterHosts: allAgentApiHosts() }
                    : {};
                await invoke("create_sandbox", { name: trimmed, existingPath: path, provider, proxy, ...network });
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
                        {m.projects_use_existing_folder()}
                    </Button>
                {:else}
                    {#if existingPath === undefined}
                        <div class="flex w-full items-center gap-2">
                            <Button variant="outline" onclick={chooseFolder}>
                                {m.projects_choose_folder()}
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
            <AdvancedSection>
                <Field.Field>
                    <Field.Label>{m.projects_runs_on_label()}</Field.Label>
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
                <Field.Field orientation="horizontal">
                    <Field.Label>
                        {m.proxy_inspect_traffic()}
                    </Field.Label>
                    <Switch
                        data-proxy-toggle
                        checked={proxy}
                        disabled={action.busy !== null}
                        onCheckedChange={(v) => (proxy = v)}
                    />
                </Field.Field>
                {#if proxy && provider === "microsandbox"}
                    <Field.Description data-proxy-msb-hint>
                        {m.create_proxy_msb_hint()}
                    </Field.Description>
                {/if}
                {#if egressSupported}
                    <Field.Field>
                        <Field.Label>
                            {m.create_network_access_label()}
                        </Field.Label>
                        <div data-egress-picker class="flex gap-2">
                            {#each [["open", m.egress_open()], ["allowlist", m.egress_allowlist()]] as const as [value, label] (value)}
                                <Button
                                    data-egress-option={value}
                                    aria-pressed={egress === value}
                                    variant={egress === value ? "default" : "outline"}
                                    size="sm"
                                    disabled={action.busy !== null}
                                    onclick={() => (egress = value)}
                                >{label}</Button>
                            {/each}
                        </div>
                        <Field.Description>
                            {#if egress === "allowlist"}
                                {m.create_egress_allowlist_hint()}
                            {:else}
                                {m.create_egress_open_hint()}
                            {/if}
                        </Field.Description>
                    </Field.Field>
                {/if}
            </AdvancedSection>
        </Field.Group>
    </Field.Set>

    <StatusRow busy={action.busy} message={action.message} error={action.error} />
</li>
