<script lang="ts">
	import { Tabs as TabsPrimitive } from "bits-ui";
	import { cn } from "$lib/utils.js";

	let {
		ref = $bindable(null),
		class: className,
		children,
		...restProps
	}: TabsPrimitive.TriggerProps = $props();
</script>

<TabsPrimitive.Trigger
	bind:ref
	data-slot="tabs-trigger"
	class={cn(
		"font-medium transition-all focus:outline-hidden disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4",
		"group-data-[variant=default]/tabs-list:rounded-md group-data-[variant=default]/tabs-list:border-2 group-data-[variant=default]/tabs-list:border-transparent group-data-[variant=default]/tabs-list:px-4 group-data-[variant=default]/tabs-list:py-1 group-data-[variant=default]/tabs-list:data-[state=active]:border-border group-data-[variant=default]/tabs-list:data-[state=active]:bg-primary group-data-[variant=default]/tabs-list:data-[state=active]:text-primary-foreground group-data-[variant=default]/tabs-list:data-[state=active]:font-semibold",
		"group-data-[variant=line]/tabs-list:border-b-2 group-data-[variant=line]/tabs-list:border-transparent group-data-[variant=line]/tabs-list:px-2 group-data-[variant=line]/tabs-list:pb-1 group-data-[variant=line]/tabs-list:data-[state=active]:border-primary group-data-[variant=line]/tabs-list:data-[state=active]:text-primary group-data-[variant=line]/tabs-list:data-[state=active]:font-semibold",
		className
	)}
	{...restProps}
>
	{@render children?.()}
</TabsPrimitive.Trigger>
