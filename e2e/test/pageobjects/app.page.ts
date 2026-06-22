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
}
