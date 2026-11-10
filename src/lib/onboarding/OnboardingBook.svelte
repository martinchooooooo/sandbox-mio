<script lang="ts">
    import { Book, Chapter } from "./book";
    import { chapters } from "./chapters";

    // The whole onboarding read: every registered chapter, back to back, in one book.
    let {
        onfinish,
        finishLabel,
        class: className,
    }: {
        onfinish?: () => void;
        finishLabel?: string;
        class?: string;
    } = $props();
</script>

<Book {onfinish} {finishLabel} class={className}>
    {#each chapters as chapter (chapter.id)}
        <Chapter title={chapter.title()}>
            <chapter.component />
        </Chapter>
    {/each}
</Book>
