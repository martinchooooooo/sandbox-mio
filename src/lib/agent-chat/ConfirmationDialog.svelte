<script lang="ts">
    import * as Item from "$lib/components/ui/item/index.js";
    import { Button } from "$lib/components/ui/button/index.js";
    import { Input } from "$lib/components/ui/input/index.js";
    import { Textarea } from "$lib/components/ui/textarea/index.js";
    import type { PendingDialog } from "./types";

    /** An extension dialog pi is blocked on, rendered in the AI Elements Confirmation pattern.
     * `answer` receives the `extension_ui_response` payload. */
    let {
        dialog,
        disabled = false,
        answer,
    }: {
        dialog: PendingDialog;
        disabled?: boolean;
        answer: (payload: Record<string, unknown>) => void;
    } = $props();

    // svelte-ignore state_referenced_locally
    let text = $state(dialog.prefill ?? "");
</script>

<Item.Root variant="outline" class="flex-col items-stretch gap-3" data-dialog={dialog.method} data-dialog-id={dialog.id}>
    <Item.Content>
        <Item.Title>{dialog.title}</Item.Title>
        {#if dialog.message}
            <Item.Description class="whitespace-pre-wrap">{dialog.message}</Item.Description>
        {/if}
    </Item.Content>
    {#if dialog.method === "input"}
        <Input
            value={text}
            oninput={(e) => (text = e.currentTarget.value)}
            placeholder={dialog.placeholder ?? ""}
            {disabled}
        />
    {:else if dialog.method === "editor"}
        <Textarea value={text} oninput={(e) => (text = e.currentTarget.value)} class="min-h-32 font-mono" {disabled} />
    {/if}
    <Item.Actions class="flex flex-wrap justify-end gap-2">
        <Button size="sm" variant="ghost" {disabled} onclick={() => answer({ cancelled: true })} data-dialog-cancel>
            Dismiss <!-- TODO(i18n) -->
        </Button>
        {#if dialog.method === "confirm"}
            <Button size="sm" variant="outline" {disabled} onclick={() => answer({ confirmed: false })} data-dialog-no>
                No <!-- TODO(i18n) -->
            </Button>
            <Button size="sm" {disabled} onclick={() => answer({ confirmed: true })} data-dialog-yes>
                Yes <!-- TODO(i18n) -->
            </Button>
        {:else if dialog.method === "select"}
            {#each dialog.options as option (option)}
                <Button size="sm" variant="outline" {disabled} onclick={() => answer({ value: option })} data-dialog-option={option}>
                    {option}
                </Button>
            {/each}
        {:else}
            <Button size="sm" {disabled} onclick={() => answer({ value: text })} data-dialog-submit>
                Submit <!-- TODO(i18n) -->
            </Button>
        {/if}
    </Item.Actions>
</Item.Root>
