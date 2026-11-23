<script lang="ts">
    import Explainer from "$lib/components/Explainer.svelte";
    import { EXPLAINERS } from "./copy";

    let { key }: { key: keyof typeof EXPLAINERS } = $props();
</script>

<Explainer
    title={EXPLAINERS[key].title}
    body={EXPLAINERS[key].body}
    data-explainer={key}
    contentProps={{ "data-explainer-card": key }}
/>
