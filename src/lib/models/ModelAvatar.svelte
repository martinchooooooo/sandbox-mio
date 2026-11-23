<script lang="ts">
    import BrainIcon from "@lucide/svelte/icons/brain";

    let { size = "lg" }: { size?: "sm" | "lg" } = $props();
</script>

<span
    class={["inline-flex flex-none items-center justify-center rounded-full bg-brain text-brain-foreground", size === "lg" ? "size-10" : "size-7"]}
    aria-hidden="true"
>
    <BrainIcon class={size === "lg" ? "size-5" : "size-4"} />
</span>
