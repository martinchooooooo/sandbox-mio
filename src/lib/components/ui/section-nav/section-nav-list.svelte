<script lang="ts">
	import { Tabs as TabsPrimitive } from "bits-ui";
	import { cn } from "$lib/utils.js";

	let {
		ref = $bindable(null),
		class: className,
		...restProps
	}: TabsPrimitive.ListProps & { "aria-label": string } = $props();
</script>

<!-- The @min-[48rem] breakpoint must equal the one in section-nav-item.svelte
(docs/features/manage-dashboard-redesign/02_dev.md, Resolved Decision 10). -->
<TabsPrimitive.List
	bind:ref
	data-slot="section-nav-list"
	class={cn(
		"border-border bg-background flex w-12 shrink-0 flex-col gap-1 self-start rounded-md border-2 p-1 shadow-md @min-[48rem]/section-nav:w-40",
		className
	)}
	{...restProps}
/>
