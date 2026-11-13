import { type Element } from "./app.page.js";
import { UI_SETTLE } from "../../lib/timeouts.js";

/**
 * The app sidebar (src/lib/workspace/AppSidebar.svelte): page entries, one entry per sandbox, "New
 * sandbox", the activity center and the collapse toggle. Page entries are driven through
 * `showPage`/`showHome` in app.page.ts; this object covers the sandbox list and the rail.
 */
class SidebarPage {
  get root(): Element {
    return $("aside[data-sidebar]");
  }

  get toggle(): Element {
    return $("button[data-sidebar-toggle]");
  }

  get sandboxList(): Element {
    return $("[data-sidebar-sandboxes]");
  }

  get newSandboxButton(): Element {
    return $("button[data-new-sandbox]");
  }

  sandboxItem(name: string): Element {
    return $(`button[data-nav-sandbox='${name}']`);
  }

  async isCollapsed(): Promise<boolean> {
    return (await (await this.root).getAttribute("data-collapsed")) === "true";
  }

  /** Clicks the collapse toggle and waits for the rail state Rust echoes back. */
  async setCollapsed(collapsed: boolean): Promise<void> {
    if ((await this.isCollapsed()) === collapsed) return;
    const t = await this.toggle;
    await t.waitForClickable({ timeout: UI_SETTLE });
    await t.click();
    await browser.waitUntil(async () => (await this.isCollapsed()) === collapsed, {
      timeout: UI_SETTLE,
      timeoutMsg: `sidebar never reported data-collapsed=${collapsed}`,
    });
  }

  async isSandboxActive(name: string): Promise<boolean> {
    const item = await this.sandboxItem(name);
    return (await item.isExisting()) && (await item.getAttribute("data-active")) === "true";
  }

  /**
   * Brings sandbox `name` into view through its sidebar entry, which restores its last view (a pane
   * or Manage), and waits for its header (docs/features/unified-sidebar-nav/02_dev.md, Resolved
   * Decision 2). The entry appears once the sandbox store has polled the sandbox in.
   */
  async selectSandbox(name: string, timeout = UI_SETTLE): Promise<void> {
    const item = await this.sandboxItem(name);
    await item.waitForClickable({ timeout, timeoutMsg: `sidebar has no clickable entry for sandbox "${name}"` });
    await item.click();
    await (await $(`div[data-sandbox-header][data-sandbox='${name}']`)).waitForDisplayed({
      timeout,
      timeoutMsg: `selecting "${name}" in the sidebar never brought its sandbox header into view`,
    });
  }

  async newSandbox(): Promise<void> {
    const btn = await this.newSandboxButton;
    await btn.waitForClickable({ timeout: UI_SETTLE });
    await btn.click();
  }
}

export default new SidebarPage();
