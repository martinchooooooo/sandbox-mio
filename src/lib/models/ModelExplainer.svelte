<script lang="ts">
    import Explainer from "$lib/components/Explainer.svelte";
    import { type ExplainerKey, explainer } from "./copy";

    let { key }: { key: ExplainerKey } = $props();
</script>

<Explainer
    title={explainer(key).title}
    body={explainer(key).body}
    data-explainer={key}
    contentProps={{ "data-explainer-card": key }}
/>
