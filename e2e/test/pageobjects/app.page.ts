import { UI_SETTLE } from "../../lib/timeouts.js";

/** A lazily-resolved WebdriverIO element chain, as returned by the global `$`. */
export type Element = ReturnType<typeof $>;

/** The app sidebar's page entries (`data-nav` in src/lib/workspace/AppSidebar.svelte). */
export type PageHref = "/projects" | "/models" | "/credentials" | "/settings" | "/intro";

/** The sidebar entry that shows routed page `href`. */
export function navButton(href: PageHref): Element {
  return $(`button[data-nav='${href}']`);
}

/**
 * Whether routed page `href` is the one showing. The URL alone cannot say so: it stays on the last
 * page while a sandbox is in view and the page is unmounted
 * (docs/features/unified-sidebar-nav/02_dev.md, Resolved Decision 9); the sidebar marks a page
 * entry active only while no sandbox is in view and the URL matches.
 */
export async function isPageShown(href: PageHref): Promise<boolean> {
  const btn = await navButton(href);
  return (await btn.isExisting()) && (await btn.getAttribute("data-active")) === "true";
}

/**
 * Shows routed page `href` through the sidebar, hiding any sandbox in view, and waits until the
 * page is the one showing. A no-op when it already is, so the page is not remounted.
 */
export async function showPage(href: PageHref, timeout = UI_SETTLE): Promise<void> {
  if (await isPageShown(href)) return;
  const btn = await navButton(href);
  await btn.waitForClickable({ timeout });
  await btn.click();
  await browser.waitUntil(() => isPageShown(href), {
    timeout,
    timeoutMsg: `sidebar entry ${href} never became the page showing (a sandbox stayed in view, or the route did not change)`,
  });
}

/**
 * Shows Home. Home cards (`li[data-sandbox]`) exist only while Home shows, so every row action
 * calls this first; `main[data-home]` is the proof Home rendered, not just that the URL matches.
 */
export async function showHome(timeout = UI_SETTLE): Promise<void> {
  const home = await $("main[data-home]");
  if (await home.isDisplayed()) return;
  await showPage("/projects", timeout);
  await (await $("main[data-home]")).waitForDisplayed({ timeout, timeoutMsg: "Home (main[data-home]) never rendered" });
}

/**
 * Base for all page objects. Concrete pages extend this and expose their selectors and actions
 * as methods so specs never reference a raw selector.
 */
export default class AppPage {
  $(selector: string): Element {
    return $(selector);
  }

  /** Shows Home; also how a spec forces the next page visit to remount that page. */
  async goHome(): Promise<void> {
    await showHome();
  }

  async showPage(href: PageHref, timeout = UI_SETTLE): Promise<void> {
    await showPage(href, timeout);
  }
}
