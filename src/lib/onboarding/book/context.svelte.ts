import { getContext, setContext } from "svelte";
import type { Scene } from "../stage";

/** One page as the book sees it. Fields are live getters, so a locale switch re-renders them. */
export interface PageEntry {
    readonly title: string;
    readonly act: string;
    readonly chapter: string;
    readonly scene: Scene | undefined;
}

/**
 * The page list and position of one `Book`. `Page` components register themselves while they
 * initialise, which Svelte does in source order, so the list order is the order pages are written.
 */
export class BookState {
    pages = $state.raw<PageEntry[]>([]);
    index = $state(0);

    get current(): PageEntry | undefined {
        return this.pages[this.index];
    }

    get isLast(): boolean {
        return this.index >= this.pages.length - 1;
    }

    register(entry: PageEntry): void {
        this.pages = [...this.pages, entry];
    }

    unregister(entry: PageEntry): void {
        this.pages = this.pages.filter((p) => p !== entry);
        this.go(this.index);
    }

    go(index: number): void {
        this.index = Math.max(0, Math.min(this.pages.length - 1, index));
    }
}

const BOOK = Symbol("story-book");
const CHAPTER = Symbol("story-chapter");
const ACT = Symbol("story-act");

export function setBook(book: BookState): void {
    setContext(BOOK, book);
}

export function getBook(): BookState {
    const book = getContext<BookState | undefined>(BOOK);
    if (!book) throw new Error("<Page> must be rendered inside a <Book>");
    return book;
}

// Labels travel as getters so a prop change in `Chapter`/`Act` reaches pages already registered.
export function setChapterLabel(label: () => string): void {
    setContext(CHAPTER, label);
}

export function chapterLabel(): () => string {
    return getContext<(() => string) | undefined>(CHAPTER) ?? (() => "");
}

export function setActLabel(label: () => string): void {
    setContext(ACT, label);
}

export function actLabel(): () => string {
    return getContext<(() => string) | undefined>(ACT) ?? (() => "");
}
