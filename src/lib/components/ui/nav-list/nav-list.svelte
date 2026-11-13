<script lang="ts">
	import type { Snippet } from "svelte";
	import type { HTMLAttributes } from "svelte/elements";
	import { cn } from "$lib/utils.js";
	import * as Tooltip from "../tooltip/index.js";
	import { setNavListContext } from "./context.js";

	let {
		collapsed = false,
		nativeTooltips = false,
		class: className,
		children,
		...restProps
	}: HTMLAttributes<HTMLElement> & {
		"aria-label": string;
		collapsed?: boolean;
		nativeTooltips?: boolean;
		children?: Snippet;
	} = $props();

	setNavListContext({
		get collapsed() {
			return collapsed;
		},
		get nativeTooltips() {
			return nativeTooltips;
		},
	});
</script>

<Tooltip.Provider>
	<nav
		data-slot="nav-list"
		data-collapsed={collapsed}
		class={cn("flex min-h-0 flex-col gap-1", className)}
		{...restProps}
	>
		{@render children?.()}
	</nav>
</Tooltip.Provider>
