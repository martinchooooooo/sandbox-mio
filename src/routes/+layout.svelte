<script>
    import "../app.css";
    import Button from "$lib/components/ui/button/button.svelte";
    import { ModeWatcher, toggleMode } from "mode-watcher";
    import { HugeiconsIcon } from "@hugeicons/svelte";
    import { SunIcon, MoonIcon } from "@hugeicons/core-free-icons";
    import { locales, getLocale, setLocale } from "$lib/paraglide/runtime.js";
    import { Toaster } from "$lib/components/ui/sonner";
    import ActivityCenter from "$lib/activity/ActivityCenter.svelte";
    import { ensureActivity } from "$lib/activity/activity.svelte";

    // Start the global activity store once, above the router, so its subscription is established
    // before any route mounts and is never torn down for the life of the webview.
    ensureActivity();

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
<Toaster />
<slot />
<ActivityCenter />
