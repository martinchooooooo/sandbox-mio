<script>
    import "../app.css";
    import Button from "$lib/components/ui/button/button.svelte";
    import { ModeWatcher, toggleMode } from "mode-watcher";
    import { HugeiconsIcon } from "@hugeicons/svelte";
    import { SunIcon, MoonIcon } from "@hugeicons/core-free-icons";
</script>

<Button onclick={toggleMode} variant="outline">
    <HugeiconsIcon icon={SunIcon}></HugeiconsIcon>
    /
    <HugeiconsIcon icon={MoonIcon}></HugeiconsIcon>
</Button>

<ModeWatcher />
<slot />
