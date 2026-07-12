<script lang="ts">
	import { Dialog as DialogPrimitive } from "bits-ui";
	import { HugeiconsIcon } from "@hugeicons/svelte";
	import { Cancel01Icon } from "@hugeicons/core-free-icons";
	import type { Snippet } from "svelte";
	import * as Dialog from "./index.js";
	import { cn, type WithoutChildrenOrChild } from "$lib/utils.js";
	import { Button } from "$lib/components/ui/button/index.js";
	let {
		ref = $bindable(null),
		class: className,
		portalProps,
		children,
		showCloseButton = true,
		...restProps
	}: WithoutChildrenOrChild<DialogPrimitive.ContentProps> & {
		portalProps?: DialogPrimitive.PortalProps;
		children: Snippet;
		showCloseButton?: boolean;
	} = $props();
</script>

<Dialog.Portal {...portalProps}>
	<Dialog.Overlay />
	<DialogPrimitive.Content
		bind:ref
		data-slot="dialog-content"
		class={cn(
			"bg-background rounded-md text-foreground data-[state=open]:animate-in data-[state=open]:fade-in-0 data-[state=open]:zoom-in-95 data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=closed]:zoom-out-95 fixed top-1/2 left-1/2 z-50 flex h-fit max-h-[80vh] w-full max-w-[calc(100%-2rem)] sm:max-w-lg -translate-x-1/2 -translate-y-1/2 flex-col gap-4 overflow-y-auto border-2 border-border p-6 shadow-md duration-300",
			className
		)}
		{...restProps}
	>
		{@render children?.()}
		{#if showCloseButton}
			<DialogPrimitive.Close data-slot="dialog-close">
				{#snippet child({ props }: { props: Record<string, unknown> })}
					<Button variant="ghost" class="absolute top-2 right-2" size="icon" {...props}>
						<HugeiconsIcon icon={Cancel01Icon} strokeWidth={2} />
						<span class="sr-only">Close</span>
					</Button>
				{/snippet}
			</DialogPrimitive.Close>
		{/if}
	</DialogPrimitive.Content>
</Dialog.Portal>
