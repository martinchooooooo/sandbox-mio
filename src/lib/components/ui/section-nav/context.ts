import { getContext, setContext } from "svelte";

const KEY = Symbol("section-nav");

/** Active section value published by `SectionNav.Root` for `SectionNav.Content`. */
export type SectionNavContext = { readonly value: string };

export function setSectionNavContext(context: SectionNavContext): void {
	setContext(KEY, context);
}

export function getSectionNavContext(): SectionNavContext {
	const context = getContext<SectionNavContext | undefined>(KEY);
	if (!context) throw new Error("SectionNav parts must render inside SectionNav.Root");
	return context;
}
