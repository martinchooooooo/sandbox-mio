<script lang="ts">
	import { HugeiconsIcon } from "@hugeicons/svelte"
	import { Loading03Icon } from '@hugeicons/core-free-icons';
	import { cn } from "$lib/utils.js";
	import type { SVGAttributes } from "svelte/elements";

	let {
		class: className,
		role = "status",
		// we add name, color, and stroke for compatibility with different icon libraries props.
		// only forward them when set, otherwise `stroke: undefined` overrides the icon's `currentColor`
		name,
		color,
		stroke,
		className: legacyClassName,
		"aria-label": ariaLabel = "Loading",
		...restProps
	}: SVGAttributes<SVGSVGElement> = $props();
</script>

<HugeiconsIcon icon={Loading03Icon} strokeWidth={2} {role} {...name != null ? { name } : {}} {...color != null ? { color } : {}} {...stroke != null ? { stroke } : {}} {...legacyClassName != null ? { className: legacyClassName } : {}} aria-label={ariaLabel} class={cn("size-4 animate-spin", className)} {...restProps} />
