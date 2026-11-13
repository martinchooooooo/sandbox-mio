<script lang="ts" module>
	import { cn, type WithElementRef } from "$lib/utils.js";
	import type { HTMLAttributes } from "svelte/elements";
	import { type VariantProps, tv } from "tailwind-variants";
	// Headings carry no font-weight: Archivo Black ships a single weight, so any bolder weight is
	// synthesised by the browser. Sizes follow the app's page scale, not the registry's.
	export const textVariants = tv({
		base: "font-head",
		variants: {
			as: {
				p: "font-sans text-base",
				li: "font-sans text-base",
				a: "font-sans text-base hover:underline underline-offset-2 decoration-primary",
				h1: "text-3xl",
				h2: "text-xl",
				h3: "text-lg",
				h4: "text-base",
				h5: "text-sm",
				h6: "text-xs",
			},
		},
		defaultVariants: {
			as: "p",
		},
	});

	export type TextAs = VariantProps<typeof textVariants>["as"];

	export type TextProps = WithElementRef<HTMLAttributes<HTMLElement>> & {
		as?: TextAs;
		class?: string;
	};
</script>

<script lang="ts">
	let {
		class: className,
		as = "p",
		children,
		ref = $bindable(null),
		...restProps
	}: TextProps = $props();
</script>

<svelte:element this={as} bind:this={ref} class={cn(textVariants({ as }), className)} {...restProps}>
	{@render children?.()}
</svelte:element>
