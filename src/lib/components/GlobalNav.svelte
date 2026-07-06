<script lang="ts">
    import * as Tabs from "$lib/components/ui/tabs";
    import { goto } from "$app/navigation";
    import { page } from "$app/state";
    import { m } from "$lib/paraglide/messages.js";

    const items = [
        { href: "/intro", label: m.nav_intro },
        { href: "/projects", label: m.nav_projects },
        { href: "/models", label: m.nav_models },
        { href: "/settings", label: m.nav_settings },
        { href: "/credentials", label: m.nav_credentials },
    ];

    const active = $derived(
        items.find((item) => item.href === page.url.pathname)?.href ?? items[0].href,
    );
</script>

<Tabs.Root value={active} onValueChange={(value) => goto(value)}>
    <Tabs.List
        variant="line"
        class="flex-wrap justify-start"
        style="height: auto;"
    >
        {#each items as item (item.href)}
            <Tabs.Trigger value={item.href}>{item.label()}</Tabs.Trigger>
        {/each}
    </Tabs.List>
</Tabs.Root>
