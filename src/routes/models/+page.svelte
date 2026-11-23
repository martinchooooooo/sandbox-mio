<script lang="ts">
    import { onMount } from "svelte";
    import { Text } from "$lib/components/ui/text";
    import ComputerSummary from "$lib/models/ComputerSummary.svelte";
    import CurrentModelCard from "$lib/models/CurrentModelCard.svelte";
    import ModelExplainer from "$lib/models/ModelExplainer.svelte";
    import ModelShelf from "$lib/models/ModelShelf.svelte";
    import { loadModels, modelsState } from "$lib/models/model.svelte";
    import { PAGE } from "$lib/models/copy";

    const st = modelsState();

    onMount(loadModels);
</script>

<main class="mx-auto w-full max-w-3xl p-6 pb-16 @max-[420px]:p-3">
    <Text as="h1" class="mb-2">{PAGE.title}</Text>
    <p class="mb-6 flex flex-wrap items-center text-sm text-muted-foreground">
        {PAGE.intro}
        <ModelExplainer key="model" />
    </p>

    <div class="stagger flex flex-col gap-6">
        <section class="flex flex-col gap-3">
            <Text as="h2">{PAGE.yourModel}</Text>
            <CurrentModelCard />
        </section>
        <ComputerSummary />
        <ModelShelf />
    </div>

    {#if st.error && !st.specs}<p class="mt-4 text-sm text-destructive">{st.error}</p>{/if}
</main>
