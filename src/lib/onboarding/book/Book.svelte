<script lang="ts">
    import type { Snippet } from "svelte";
    import Button from "$lib/components/ui/button/button.svelte";
    import Stage from "./Stage.svelte";
    import { BookState, setBook } from "./context.svelte";

    // The reader: an animated stage on top, the current page's text below, and Back / dots / Next.
    // Pages are whatever `<Page>` components render inside `children`, in source order.
    let {
        children,
        onfinish,
        finishLabel = "Finish", // TODO(i18n)
        auto = false,
        speed = 1,
        class: className = "",
    }: {
        children: Snippet;
        /** Called from the last page's forward button. Without it the button starts over. */
        onfinish?: () => void;
        finishLabel?: string;
        auto?: boolean;
        speed?: number;
        /** Extra classes for the card, e.g. a `max-h-*` that keeps the navigation in view. */
        class?: string;
    } = $props();

    const book = new BookState();
    setBook(book);

    const page = $derived(book.current);

    function forward() {
        if (!book.isLast) book.go(book.index + 1);
        else if (onfinish) onfinish();
        else book.go(0);
    }

    function onkeydown(e: KeyboardEvent) {
        const target = e.target as HTMLElement | null;
        if (target?.closest("input, textarea, [contenteditable]")) return;
        if (e.key === "ArrowRight" && !book.isLast) book.go(book.index + 1);
        if (e.key === "ArrowLeft") book.go(book.index - 1);
    }
</script>

<svelte:window {onkeydown} />

<article
    class={["flex w-full max-w-[980px] flex-col overflow-hidden rounded-md border-2 bg-card text-card-foreground shadow-md", className]}
    data-story-book
    data-story-page={book.index}
    data-story-pages={book.pages.length}
>
    {#if page?.chapter}
        <header class="flex shrink-0 items-center justify-between gap-3 border-b-2 px-5 py-2 text-sm">
            <span class="font-head">{page.chapter}</span>
            <span class="text-muted-foreground tabular-nums">{book.index + 1} / {book.pages.length}</span>
        </header>
    {/if}

    <Stage scene={page?.scene} {auto} {speed} />

    <!-- The text scrolls inside the card, so a height-capped book never loses its navigation. -->
    <section class="min-h-0 flex-1 overflow-y-auto px-8 pb-1.5 pt-4 @max-[640px]:px-4">
        {@render children()}
    </section>

    <nav class="flex shrink-0 items-center justify-between gap-3 px-8 pb-4 pt-2 @max-[640px]:px-4">
        <!-- TODO(i18n) -->
        <Button variant="outline" disabled={book.index === 0} onclick={() => book.go(book.index - 1)} data-story-back>← Back</Button>
        <div class="flex flex-wrap justify-center gap-2">
            {#each book.pages as entry, i (entry)}
                <button
                    type="button"
                    class={[
                        "size-3.5 cursor-pointer rounded-full border-2 border-border p-0 transition-colors",
                        i === book.index ? "bg-primary" : "bg-background hover:bg-muted",
                        // A wider gap marks where a new act starts.
                        i > 0 && entry.act !== book.pages[i - 1].act && "ml-2.5",
                    ]}
                    title={`${i + 1}. ${entry.title}`}
                    aria-label={`${i + 1}. ${entry.title}`}
                    aria-current={i === book.index ? "page" : undefined}
                    onclick={() => book.go(i)}
                    data-story-dot={i}
                ></button>
            {/each}
        </div>
        <Button onclick={forward} data-story-next>
            <!-- TODO(i18n) -->
            {#if !book.isLast}Next →{:else if onfinish}{finishLabel}{:else}Start over ↺{/if}
        </Button>
    </nav>
</article>
