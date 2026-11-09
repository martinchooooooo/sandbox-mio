<script lang="ts">
	import { Tabs as TabsPrimitive } from "bits-ui";
	import type { Snippet } from "svelte";
	import { cn } from "$lib/utils.js";
	import * as Tooltip from "../tooltip/index.js";

	let {
		ref = $bindable(null),
		value,
		label,
		icon,
		class: className,
		...restProps
	}: Omit<TabsPrimitive.TriggerProps, "children" | "child"> & {
		value: string;
		label: string;
		icon: Snippet;
	} = $props();

	let labelEl = $state<HTMLElement | null>(null);
	let tipOpen = $state(false);

	// The tooltip is portalled out of the container, so only the label's own size says whether the
	// list is collapsed. An sr-only label renders 1px wide.
	$effect(() => {
		if (tipOpen && (labelEl === null || labelEl.getBoundingClientRect().width > 1)) tipOpen = false;
	});
</script>

<!-- The @min-[48rem] breakpoint must equal the one in section-nav-list.svelte
(docs/features/manage-dashboard-redesign/02_dev.md, Resolved Decision 10). -->
<Tooltip.Root bind:open={tipOpen}>
	<Tooltip.Trigger>
		{#snippet child({ props })}
			<TabsPrimitive.Trigger
				bind:ref
				{...props}
				{...restProps}
				{value}
				data-slot="section-nav-item"
				data-section-nav-item={value}
				class={cn(
					"flex h-9 items-center justify-center gap-2 rounded-md border-2 border-transparent px-1.5 text-sm font-medium transition-all focus:outline-hidden focus-visible:shadow-xs disabled:pointer-events-none disabled:opacity-50 @min-[48rem]/section-nav:justify-start @min-[48rem]/section-nav:px-2.5 [&_svg]:pointer-events-none [&_svg]:shrink-0",
					"data-[state=active]:border-border data-[state=active]:bg-primary data-[state=active]:text-primary-foreground data-[state=active]:font-semibold",
					className
				)}
			>
				{@render icon()}
				<span bind:this={labelEl} class="sr-only truncate @min-[48rem]/section-nav:not-sr-only">
					{label}
				</span>
			</TabsPrimitive.Trigger>
		{/snippet}
	</Tooltip.Trigger>
	<Tooltip.Content side="right">{label}</Tooltip.Content>
</Tooltip.Root>
