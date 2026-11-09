<script lang="ts">
	import { Tabs as TabsPrimitive } from "bits-ui";
	import { cn } from "$lib/utils.js";
	import * as Tooltip from "../tooltip/index.js";
	import { setSectionNavContext } from "./context.js";

	let {
		ref = $bindable(null),
		value = $bindable(""),
		class: className,
		children,
		...restProps
	}: TabsPrimitive.RootProps = $props();

	setSectionNavContext({
		get value() {
			return value;
		},
	});
</script>

<!-- Manual activation: arrowing across items must not mount, and so fetch for, each section
(docs/features/manage-dashboard-redesign/02_dev.md, Resolved Decision 11). -->
<Tooltip.Provider>
	<TabsPrimitive.Root
		bind:ref
		bind:value
		orientation="vertical"
		activationMode="manual"
		data-slot="section-nav"
		data-section-nav
		class={cn("@container/section-nav flex h-full min-h-0 w-full gap-4", className)}
		{...restProps}
	>
		{@render children?.()}
	</TabsPrimitive.Root>
</Tooltip.Provider>
