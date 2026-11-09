<script lang="ts">
	import { Tabs as TabsPrimitive } from "bits-ui";
	import { cn } from "$lib/utils.js";
	import { getSectionNavContext } from "./context.js";

	let {
		ref = $bindable(null),
		value,
		class: className,
		children,
		...restProps
	}: TabsPrimitive.ContentProps = $props();

	const nav = getSectionNavContext();
</script>

<!-- bits-ui Tabs.Content renders its children even while hidden, so the gate lives here
(docs/features/manage-dashboard-redesign/02_dev.md, Common Gotchas). -->
<TabsPrimitive.Content
	bind:ref
	{value}
	data-slot="section-nav-content"
	class={cn("data-[state=active]:animate-fade-in min-w-0 flex-1 overflow-y-auto outline-none", className)}
	{...restProps}
>
	{#if nav.value === value}
		{@render children?.()}
	{/if}
</TabsPrimitive.Content>
