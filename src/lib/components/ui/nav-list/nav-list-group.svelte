<script lang="ts">
	import type { Snippet } from "svelte";
	import { cn } from "$lib/utils.js";
	import { getNavListContext } from "./context.js";

	let {
		label,
		class: className,
		children,
		...restProps
	}: { label?: string; class?: string; children?: Snippet; [key: `data-${string}`]: unknown } =
		$props();

	const nav = getNavListContext();
</script>

<div role="group" aria-label={label} data-slot="nav-list-group" class={cn("flex flex-col gap-1", className)} {...restProps}>
	{#if label}
		<p class={nav.collapsed ? "sr-only" : "text-muted-foreground px-2.5 pt-2 text-xs font-medium uppercase tracking-wide"}>
			{label}
		</p>
	{/if}
	{@render children?.()}
</div>
