<script lang="ts">
    import Explainer from "$lib/components/Explainer.svelte";
    import type { Role } from "./anatomy";
    import { ROLE_COPY } from "./anatomy-copy";

    let { role }: { role: Role } = $props();
    const copy = $derived(ROLE_COPY[role]);
</script>

<Explainer
    title={copy.title}
    body={copy.body}
    data-anatomy-info={role}
    contentProps={{ "data-anatomy-explainer": "", "data-explainer-role": role }}
/>
