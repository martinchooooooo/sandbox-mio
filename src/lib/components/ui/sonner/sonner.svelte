<script lang="ts">
	import { Toaster as Sonner, type ToasterProps as SonnerProps } from "svelte-sonner";
	import { mode } from "mode-watcher";
	import { HugeiconsIcon } from "@hugeicons/svelte";
	import { Loading03Icon } from "@hugeicons/core-free-icons";
	import { CheckmarkCircle02Icon } from "@hugeicons/core-free-icons";
	import { MultiplicationSignCircleIcon } from "@hugeicons/core-free-icons";
	import { InformationCircleIcon } from "@hugeicons/core-free-icons";
	import { Alert02Icon } from "@hugeicons/core-free-icons";

	let { ...restProps }: SonnerProps = $props();
</script>

<Sonner
	theme={mode.current}
	class="toaster group"
	toastOptions={{
		classes: {
			toast:
				"h-auto rounded-md w-full p-4 bg-background border-2 border-border group toast group-[.toaster]:bg-background group-[.toaster]:text-foreground group-[.toaster]:border-border flex items-center relative",
			description: "group-[.toast]:!text-muted-foreground ml-2 text-sm font-sans",
			actionButton:
				"group-[.toast]:bg-primary rounded-md group-[.toast]:text-primary-foreground py-1 px-2 bg-background border-border shadow-md border-2 ml-auto h-fit min-w-fit transition-all hover:translate-y-1 hover:shadow-none font-medium",
			cancelButton:
				"group-[.toast]:bg-muted rounded-md group-[.toast]:text-foreground py-1 px-2 text-sm bg-background border-border shadow-md border-2 ml-auto h-fit min-w-fit transition-all hover:translate-y-1 hover:shadow-none font-medium",
			title: "ml-2 font-sans font-medium",
			closeButton:
				"absolute rounded-md bg-background border border-border shadow-sm -top-1 -left-1 p-0.5 transition-all hover:translate-y-0.5 hover:shadow-none",
		},
		unstyled: true,
	}}
	{...restProps}
>
	{#snippet loadingIcon()}
		<HugeiconsIcon icon={Loading03Icon} strokeWidth={2} class="size-4 animate-spin" />
	{/snippet}
	{#snippet successIcon()}
		<HugeiconsIcon icon={CheckmarkCircle02Icon} strokeWidth={2} class="size-4" />
	{/snippet}
	{#snippet errorIcon()}
		<HugeiconsIcon icon={MultiplicationSignCircleIcon} strokeWidth={2} class="size-4" />
	{/snippet}
	{#snippet infoIcon()}
		<HugeiconsIcon icon={InformationCircleIcon} strokeWidth={2} class="size-4" />
	{/snippet}
	{#snippet warningIcon()}
		<HugeiconsIcon icon={Alert02Icon} strokeWidth={2} class="size-4" />
	{/snippet}
</Sonner>
