<script lang="ts">
    import { Streamdown } from "svelte-streamdown";
    import Code from "svelte-streamdown/code";
    import { openUrl } from "@tauri-apps/plugin-opener";

    let { content, streaming = false }: { content: string; streaming?: boolean } = $props();

    // A link followed inside the main webview would navigate the whole app away; open it in the
    // system browser instead.
    function onclick(event: MouseEvent) {
        const anchor = (event.target as HTMLElement | null)?.closest("a[href]");
        if (!anchor) return;
        event.preventDefault();
        void openUrl((anchor as HTMLAnchorElement).href);
    }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="min-w-0 text-sm leading-relaxed" {onclick}>
    <!-- Raw HTML stays off, images never load, links only to https and mailto
         (docs/features/native-agent-ui/02_dev.md, Resolved Decision 10). -->
    <Streamdown
        {content}
        static={!streaming}
        baseTheme="shadcn"
        components={{ code: Code }}
        allowedLinkPrefixes={["https://", "mailto:"]}
        allowedImagePrefixes={[]}
        codeBlockMaxHeight="24rem"
    />
</div>
