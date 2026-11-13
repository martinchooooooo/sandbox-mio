import { getContext, setContext } from "svelte";

const KEY = Symbol("nav-list");

/**
 * Published by `NavList.Root` for its groups and items. `collapsed` hides labels behind
 * tooltips; `nativeTooltips` swaps the DOM tooltip for a `title`, which the webview draws as an
 * OS popup above sibling native webviews.
 */
export type NavListContext = { readonly collapsed: boolean; readonly nativeTooltips: boolean };

export function setNavListContext(context: NavListContext): void {
	setContext(KEY, context);
}

export function getNavListContext(): NavListContext {
	const context = getContext<NavListContext | undefined>(KEY);
	if (!context) throw new Error("NavList parts must render inside NavList.Root");
	return context;
}
