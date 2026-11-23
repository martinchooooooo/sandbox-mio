<script lang="ts">
	import { Tabs as TabsPrimitive } from "bits-ui";
	import type { Snippet } from "svelte";
	import { cn } from "$lib/utils.js";
	import { navItemBase } from "./styles.js";

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
</script>

<TabsPrimitive.Trigger
	bind:ref
	{...restProps}
	{value}
	data-slot="section-nav-item"
	data-section-nav-item={value}
	class={cn(
		navItemBase,
		"shrink-0 justify-start px-2 whitespace-nowrap",
		"data-[state=active]:border-border data-[state=active]:bg-primary data-[state=active]:text-primary-foreground data-[state=active]:font-semibold",
		className
	)}
>
	<span class="hidden @[52rem]/tabs:contents">{@render icon()}</span>
	{label}
</TabsPrimitive.Trigger>
