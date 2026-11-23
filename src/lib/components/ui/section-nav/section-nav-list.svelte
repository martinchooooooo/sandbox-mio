<script lang="ts">
	import { Tabs as TabsPrimitive } from "bits-ui";
	import { cn } from "$lib/utils.js";

	let {
		ref = $bindable(null),
		class: className,
		...restProps
	}: TabsPrimitive.ListProps & { "aria-label": string } = $props();
</script>

<!-- Labels always show; icons drop below 52rem so the strip fits one row, and wrapping is the last resort
(docs/features/workspace-ux/02_dev.md). -->
<TabsPrimitive.List
	bind:ref
	data-slot="section-nav-list"
	class={cn("@container/tabs border-border flex w-full shrink-0 flex-wrap items-center gap-1 border-b-2 pb-2", className)}
	{...restProps}
/>
