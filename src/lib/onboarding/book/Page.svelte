<script lang="ts">
    import { onDestroy, type Snippet } from "svelte";
    import type { Scene } from "../stage";
    import { actLabel, chapterLabel, getBook, type PageEntry } from "./context.svelte";

    // One page of a book: a title, an optional animated scene, and the explanation as children.
    // It registers with the enclosing `Book` and renders only while it is the current page.
    let {
        title,
        scene,
        children,
    }: {
        title: string;
        /** The stage animation for this page; omit it for a text-only page. */
        scene?: Scene;
        children?: Snippet;
    } = $props();

    const book = getBook();
    const chapter = chapterLabel();
    const act = actLabel();

    const entry: PageEntry = {
        get title() { return title; },
        get act() { return act(); },
        get chapter() { return chapter(); },
        get scene() { return scene; },
    };
    // During init, not in onMount: mount order runs children first and would shuffle the pages.
    book.register(entry);
    onDestroy(() => book.unregister(entry));
</script>

{#if book.current === entry}
    <div class="animate-enter" data-story-leaf>
        {#if entry.act}
            <p class="mb-2.5 inline-block rounded-full border-2 border-border bg-accent px-2.5 py-0.5 text-xs font-bold uppercase tracking-widest text-accent-foreground">
                {entry.act}
            </p>
        {/if}
        <h1 class="mb-2.5 font-head text-3xl leading-tight @max-[640px]:text-2xl">{title}</h1>
        <div class="max-w-[64ch] text-[17px] leading-relaxed [&_p]:mb-2.5">
            {@render children?.()}
        </div>
    </div>
{/if}
