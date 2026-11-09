<script lang="ts">
	import { Collapsible as CollapsiblePrimitive } from "bits-ui";
	import { cn } from "$lib/utils.js";
	let { ref = $bindable(null), class: className, ...restProps }: CollapsiblePrimitive.ContentProps = $props();
</script>

<!-- Enter-only fade: a height animation would need overflow clipping, which cuts off the hard
drop-shadows of controls inside. -->
<CollapsiblePrimitive.Content
	bind:ref
	data-slot="collapsible-content"
	class={cn("data-[state=open]:animate-fade-in", className)}
	{...restProps}
/>
