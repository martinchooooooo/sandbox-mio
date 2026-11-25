<script lang="ts">
    import { invoke } from "@tauri-apps/api/core";
    import Button from "$lib/components/ui/button/button.svelte";
    import * as Dialog from "$lib/components/ui/dialog/index.js";
    import { parseError } from "$lib/errors";
    import type { ApplyReport, Proposal } from "./net-policy";

    let {
        open = $bindable(false),
        sandboxName,
        baseRevision,
        proposal,
        onresolved,
    }: {
        open: boolean;
        sandboxName: string;
        baseRevision: number;
        proposal: Proposal;
        onresolved: (report: ApplyReport | null) => void;
    } = $props();

    let busy = $state(false);
    let error = $state<string | null>(null);

    // The backend already sorts widening changes first; this only keeps that order explicit.
    const ordered = $derived([
        ...proposal.changes.filter((c) => c.widens),
        ...proposal.changes.filter((c) => !c.widens),
    ]);

    async function run(command: () => Promise<ApplyReport | null>) {
        busy = true;
        error = null;
        try {
            const report = await command();
            open = false;
            onresolved(report);
        } catch (e) {
            error = parseError(e).message;
        } finally {
            busy = false;
        }
    }

    // Sends exactly the policy displayed, never a re-read of the workspace file.
    const apply = () =>
        run(() =>
            invoke<ApplyReport>("apply_sandbox_policy", {
                name: sandboxName,
                policy: $state.snapshot(proposal.policy),
                baseRevision,
            }),
        );
    const discard = () =>
        run(async () => {
            await invoke("discard_net_policy_proposal", { name: sandboxName });
            return null;
        });
</script>

<Dialog.Root bind:open>
    <Dialog.Content class="max-h-[80vh] overflow-y-auto" data-net-policy-proposal-dialog>
        <Dialog.Header>
            <Dialog.Title>Review network policy changes <!-- TODO(i18n) --></Dialog.Title>
            <Dialog.Description>
                The workspace file differs from the policy this sandbox runs. Nothing below is in force until you apply it. <!-- TODO(i18n) -->
            </Dialog.Description>
        </Dialog.Header>
        <ul class="flex flex-col gap-1.5">
            {#each ordered as change, i (i)}
                {@const sentence = change.risk}
                <li
                    class="flex flex-col gap-0.5 rounded border px-2 py-1 text-xs {change.widens ? 'border-destructive text-destructive' : ''}"
                    data-proposal-change={change.kind}
                    data-widens={change.widens}
                >
                    <span>{change.text}</span>
                    {#if sentence}
                        <span class="font-bold" data-proposal-risk>{sentence}</span>
                    {/if}
                </li>
            {/each}
        </ul>
        {#if error}
            <span class="text-xs text-red-500" data-proposal-error>{error}</span>
        {/if}
        <Dialog.Footer>
            <Button variant="outline" disabled={busy} onclick={() => (open = false)} data-proposal-cancel>Cancel <!-- TODO(i18n) --></Button>
            <Button variant="outline" disabled={busy} onclick={discard} data-proposal-discard>Discard changes <!-- TODO(i18n) --></Button>
            <Button disabled={busy} onclick={apply} data-proposal-apply>Apply changes <!-- TODO(i18n) --></Button>
        </Dialog.Footer>
    </Dialog.Content>
</Dialog.Root>
