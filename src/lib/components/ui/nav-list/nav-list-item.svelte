<script lang="ts">
	import type { Snippet } from "svelte";
	import type { HTMLButtonAttributes } from "svelte/elements";
	import { cn } from "$lib/utils.js";
	import * as Tooltip from "../tooltip/index.js";
	import { navItemBase } from "../section-nav/styles.js";
	import { getNavListContext } from "./context.js";

	let {
		ref = $bindable(null),
		label,
		active = false,
		icon,
		trailing,
		class: className,
		...restProps
	}: Omit<HTMLButtonAttributes, "children"> & {
		ref?: HTMLButtonElement | null;
		label: string;
		active?: boolean;
		icon: Snippet;
		/** Badge or marker after the label; pinned to the icon's corner in the rail. */
		trailing?: Snippet;
	} = $props();

	const nav = getNavListContext();
	const domTooltip = $derived(nav.collapsed && !nav.nativeTooltips);
</script>

{#snippet item(props: Record<string, unknown>)}
	<button
		bind:this={ref}
		type="button"
		{...props}
		{...restProps}
		data-slot="nav-list-item"
		data-active={active}
		aria-current={active ? "page" : undefined}
		aria-label={nav.collapsed ? label : undefined}
		title={nav.collapsed && nav.nativeTooltips ? label : undefined}
		class={cn(
			navItemBase,
			"relative w-full hover:border-border",
			nav.collapsed ? "justify-center px-0" : "justify-start px-2.5",
			active && "border-border bg-primary text-primary-foreground font-semibold",
			className
		)}
	>
		{@render icon()}
		{#if !nav.collapsed}
			<span class="min-w-0 flex-1 truncate text-left">{label}</span>
		{/if}
		{#if trailing}
			<span class={nav.collapsed ? "absolute -right-1 -top-1" : "shrink-0"}>
				{@render trailing()}
			</span>
		{/if}
	</button>
{/snippet}

{#if domTooltip}
	<Tooltip.Root>
		<Tooltip.Trigger>
			{#snippet child({ props })}
				{@render item(props)}
			{/snippet}
		</Tooltip.Trigger>
		<Tooltip.Content side="right">{label}</Tooltip.Content>
	</Tooltip.Root>
{:else}
	{@render item({})}
{/if}
