<script lang="ts">
    import Button from "$lib/components/ui/button/button.svelte";
    import { toggleMode } from "mode-watcher";
    import { HugeiconsIcon } from "@hugeicons/svelte";
    import { SunIcon, MoonIcon } from "@hugeicons/core-free-icons";
    import { locales, getLocale, setLocale } from "$lib/paraglide/runtime.js";
    import { m } from "$lib/paraglide/messages.js";

    function cycleLocale() {
        const current = getLocale();
        const idx = locales.indexOf(current);
        setLocale(locales[(idx + 1) % locales.length]);
    }
</script>

<div class="flex gap-2">
    <Button href="/credentials" variant="outline">{m.nav_credentials()}</Button>
    <Button onclick={toggleMode} variant="outline">
        <HugeiconsIcon icon={SunIcon}></HugeiconsIcon>
        /
        <HugeiconsIcon icon={MoonIcon}></HugeiconsIcon>
    </Button>
    <Button onclick={cycleLocale} variant="outline">
        {getLocale().toUpperCase()}
    </Button>
</div>
