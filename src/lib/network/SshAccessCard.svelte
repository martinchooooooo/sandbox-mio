<script lang="ts">
    import { untrack } from "svelte";
    import { writeText } from "@tauri-apps/plugin-clipboard-manager";
    import { toast } from "svelte-sonner";
    import { HugeiconsIcon } from "@hugeicons/svelte";
    import { Copy01Icon, Refresh01Icon, Tick02Icon } from "@hugeicons/core-free-icons";
    import Button from "$lib/components/ui/button/button.svelte";
    import AdvancedSection from "$lib/components/AdvancedSection.svelte";
    import { createActionRunner } from "$lib/actions.svelte";
    import { parseError } from "$lib/errors";
    import { sandboxSshAccess, type SshAccess } from "$lib/api/ssh";
    import { deploymentFor, liveStatus, sandboxesList } from "$lib/sandbox/state.svelte";

    let {
        sandboxName,
        action,
    }: {
        sandboxName: string;
        workspacePath: string;
        action: ReturnType<typeof createActionRunner>;
    } = $props();

    /** The base image's `WORKDIR`; mise shims resolve tool versions only from inside it
     *  (docs/features/host-tool-ssh-access/spike/s2-path.md). */
    const WORKSPACE_DIR = "/root/work";

    // Own runner, not the dashboard's: a background fetch must neither disable the dashboard's
    // actions nor clear an error one of them is showing.
    const fetcher = createActionRunner();
    let loaded = $state<{ name: string; access: SshAccess } | null>(null);
    let requestSeq = 0;
    let copiedKey = $state<string | null>(null);
    let copiedTimer: ReturnType<typeof setTimeout> | undefined;

    const access = $derived(loaded?.name === sandboxName ? loaded.access : null);
    const entry = $derived(sandboxesList().find((p) => p.name === sandboxName));
    const status = $derived(entry ? liveStatus(entry) : undefined);
    const deployedHost = $derived(deploymentFor(sandboxName)?.host);
    // `running` is false for every deployed sandbox, whatever its remote state, so the deployment
    // overlay decides first (docs/features/host-tool-ssh-access/04_improvements.md).
    const connection = $derived<"loading" | "deployed" | "running" | "stopped">(
        deployedHost ? "deployed" : !access ? "loading" : access.running ? "running" : "stopped",
    );

    async function load(name: string) {
        const seq = ++requestSeq;
        await fetcher.run("fetch", async () => {
            const result = await sandboxSshAccess(name);
            if (seq === requestSeq) loaded = { name, access: result };
        });
    }

    // Re-fetched on start/stop and deploy/undeploy too: both change `running`, and a move between
    // machines rewrites the block's ProxyCommand.
    $effect(() => {
        const name = sandboxName;
        void status;
        void deployedHost;
        untrack(() => load(name));
    });

    $effect(() => () => clearTimeout(copiedTimer));

    async function copy(key: string, value: string) {
        try {
            await writeText(value);
            copiedKey = key;
            clearTimeout(copiedTimer);
            copiedTimer = setTimeout(() => (copiedKey = null), 1500);
        } catch (e) {
            toast.error(parseError(e).message);
        }
    }
</script>

{#snippet copyButton(key: string, value: string, label: string)}
    <Button variant="ghost" size="icon" title={label} aria-label={label} data-ssh-copy={key}
        onclick={() => copy(key, value)}>
        <HugeiconsIcon icon={copiedKey === key ? Tick02Icon : Copy01Icon} size={14} />
    </Button>
{/snippet}

{#snippet detailRow(key: string, label: string, value: string)}
    <div class="flex items-center gap-2 rounded bg-muted px-2 py-1 text-xs">
        <span class="text-muted-foreground w-24 shrink-0">{label}</span>
        <span class="flex-1 truncate font-mono select-all" title={value} data-ssh-value={key}>{value}</span>
        <!-- TODO(i18n) -->
        {@render copyButton(key, value, `Copy ${label}`)}
    </div>
{/snippet}

<div class="flex flex-col gap-1.5" data-ssh-access={sandboxName} data-ssh-state={connection}>
    <div class="flex items-center justify-between gap-2">
        <!-- TODO(i18n) -->
        <span class="text-muted-foreground text-xs shrink-0">Connect from desktop apps</span>
        <!-- TODO(i18n) -->
        <Button variant="ghost" size="sm" data-ssh-refresh title="Refresh"
            loading={fetcher.busy === "fetch"} disabled={fetcher.busy !== null || action.busy !== null}
            onclick={() => load(sandboxName)}>
            <HugeiconsIcon icon={Refresh01Icon} size={14} />
            Refresh<!-- TODO(i18n) -->
        </Button>
    </div>

    <!-- Wording confirmed by spike S1 against Claude Desktop (docs/features/host-tool-ssh-access/02_dev.md,
         Phase 0): the alias supplies the connection, the key and the pinned host key. -->
    <p class="text-xs">
        In your app's "Add SSH connection" (Claude Desktop, VS Code Remote-SSH), enter the SSH host name below. Leave the port empty (or 22) and leave the key field empty: the SSH host already supplies the connection and the key.<!-- TODO(i18n) -->
    </p>

    {#if access}
        <div class="flex items-center gap-2 rounded bg-muted px-2 py-1 text-xs">
            <!-- TODO(i18n) -->
            <span class="text-muted-foreground w-24 shrink-0">SSH host</span>
            <span class="flex-1 truncate font-mono font-semibold select-all" data-ssh-alias>{access.alias}</span>
            <!-- TODO(i18n) -->
            {@render copyButton("alias", access.alias, "Copy SSH host")}
        </div>
    {/if}

    {#if connection === "deployed"}
        <p class="text-muted-foreground text-xs" data-ssh-state-text>
            Deployed to {deployedHost}. The SSH host connects to the sandbox there, so it must be running on that machine.<!-- TODO(i18n) -->
        </p>
    {:else if connection === "stopped"}
        <p class="text-xs text-amber-600" data-ssh-state-text>
            This sandbox is stopped. Start it with the start button at the top of the page before connecting.<!-- TODO(i18n) -->
        </p>
    {:else if connection === "running"}
        <p class="text-muted-foreground text-xs" data-ssh-state-text>
            Running and ready to connect.<!-- TODO(i18n) -->
        </p>
    {/if}

    {#if access}
        <div class="flex items-center gap-2 text-xs">
            <p class="text-muted-foreground flex-1">
                Once connected, open the folder <span class="font-mono">{WORKSPACE_DIR}</span>: tools installed through this app only work from inside it.<!-- TODO(i18n) -->
            </p>
            <!-- TODO(i18n) -->
            {@render copyButton("workdir", WORKSPACE_DIR, "Copy folder path")}
        </div>

        <!-- TODO(i18n) -->
        <AdvancedSection label="Details" data-ssh-details>
            <div class="flex flex-col gap-1">
                <!-- TODO(i18n) -->
                {@render detailRow("identity", "Identity file", access.identityPath)}
                <!-- TODO(i18n) -->
                {@render detailRow("known-hosts", "known_hosts", access.knownHostsPath)}
                <!-- TODO(i18n) -->
                {@render detailRow("proxy-command", "ProxyCommand", access.proxyCommand)}
                <p class="text-muted-foreground text-xs">
                    For a client that cannot read ~/.ssh/config. The SSH host stays current when the sandbox is deployed or brought back; a copied ProxyCommand does not.<!-- TODO(i18n) -->
                </p>
            </div>
        </AdvancedSection>
    {/if}

    {#if fetcher.error}
        <span class="text-xs text-red-500">{fetcher.error}</span>
    {/if}
</div>
