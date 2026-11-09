<script lang="ts">
	import { Separator } from "../separator/index.js";
	import { cn } from "$lib/utils.js";

	let { class: className, ...restProps }: { class?: string } = $props();
</script>

<Separator class={cn("my-1", className)} {...restProps} />
