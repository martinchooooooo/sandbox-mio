import { UI_SETTLE } from "../../lib/timeouts.js";

/** A lazily-resolved WebdriverIO element chain, as returned by the global `$`. */
export type Element = ReturnType<typeof $>;

/**
 * Base for all page objects. Concrete pages extend this and expose their selectors and actions
 * as methods so specs never reference a raw selector.
 */
export default class AppPage {
  $(selector: string): Element {
    return $(selector);
  }

  // Waits for the destination nav before returning: a following open() would otherwise read the
  // pre-transition URL and skip its own nav click.
  async goHome(): Promise<void> {
    if (/\/(models|settings|intro|projects)(\/|$)/.test(await browser.getUrl())) {
      const projectsTab = await $("[role='tab'][data-value='/projects']");
      await projectsTab.waitForDisplayed({ timeout: UI_SETTLE });
      await projectsTab.click();
      await $("[role='tab'][data-value='/settings']").waitForDisplayed({ timeout: UI_SETTLE });
    }
  }
}
