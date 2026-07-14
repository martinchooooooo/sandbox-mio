// A lazily-resolved WebdriverIO element chain, as returned by the global `$`.
export type Element = ReturnType<typeof $>;

/**
 * Base for all page objects. Concrete pages extend this and expose their selectors and actions
 * as methods so specs never reference a raw selector.
 */
export default class AppPage {
  $(selector: string): Element {
    return $(selector);
  }

  // Navigate to the home page via the global nav's project tab (GlobalNav is layout-mounted, so
  // it's present on every route). Waits for the home nav to confirm the transition is complete —
  // without this, a subsequent open() call may read a stale URL (still showing the previous
  // route) and skip its own nav click, leaving the page in a transitioning state where key
  // elements are not yet displayed.
  async goHome(): Promise<void> {
    if (/\/(models|settings|intro|projects)(\/|$)/.test(await browser.getUrl())) {
      const projectsTab = await $("[role='tab'][data-value='/projects']");
      await projectsTab.waitForDisplayed({ timeout: 5000 });
      await projectsTab.click();
      await $("[role='tab'][data-value='/settings']").waitForDisplayed({ timeout: 5000 });
    }
  }
}
