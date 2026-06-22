<script>
    import "../app.css";
    import Button from "$lib/components/ui/button/button.svelte";
    import { ModeWatcher, toggleMode } from "mode-watcher";
    import { HugeiconsIcon } from "@hugeicons/svelte";
    import { SunIcon, MoonIcon } from "@hugeicons/core-free-icons";
    import { locales, getLocale, setLocale } from "$lib/paraglide/runtime.js";

    function cycleLocale() {
        const current = getLocale();
        const idx = locales.indexOf(current);
        setLocale(locales[(idx + 1) % locales.length]);
    }
</script>

<div class="flex gap-2">
    <Button onclick={toggleMode} variant="outline">
        <HugeiconsIcon icon={SunIcon}></HugeiconsIcon>
        /
        <HugeiconsIcon icon={MoonIcon}></HugeiconsIcon>
    </Button>
    <Button onclick={cycleLocale} variant="outline">
        {getLocale().toUpperCase()}
    </Button>
</div>

<ModeWatcher />
<slot />
