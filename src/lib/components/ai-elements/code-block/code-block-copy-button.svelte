<script lang="ts">
	import { onDestroy } from "svelte";
	import { HugeiconsIcon } from "@hugeicons/svelte"
	import { Tick02Icon } from '@hugeicons/core-free-icons';
	import { Copy01Icon } from '@hugeicons/core-free-icons';
	import { Button, type ButtonProps } from "$lib/components/ui/button/index.js";
	import { cn } from "$lib/utils.js";

	let {
		ref = $bindable(null),
		class: className,
		value,
		variant = "ghost",
		size = "icon",
		onclick,
		...restProps
	}: Omit<ButtonProps, "onclick"> & {
		value: string;
		onclick?: (e: MouseEvent) => void;
	} = $props();

	let copied = $state(false);
	let timeout: ReturnType<typeof setTimeout> | undefined;

	function copy() {
		navigator.clipboard
			.writeText(value)
			.then(() => {
				copied = true;
				clearTimeout(timeout);
				timeout = setTimeout(() => (copied = false), 2000);
			})
			.catch(() => {});
	}

	onDestroy(() => clearTimeout(timeout));
</script>

<Button
	bind:ref
	data-slot="code-block-copy-button"
	data-copied={copied || undefined}
	{variant}
	{size}
	aria-label={copied ? "Copied" : "Copy"}
	class={cn("size-7", className)}
	onclick={(e) => {
		onclick?.(e);
		copy();
	}}
	{...restProps}
>
	{#if copied}
		<HugeiconsIcon icon={Tick02Icon} strokeWidth={2}  />
	{:else}
		<HugeiconsIcon icon={Copy01Icon} strokeWidth={2}  />
	{/if}
</Button>
<span aria-live="polite" class="sr-only">{copied ? "Copied" : ""}</span>
