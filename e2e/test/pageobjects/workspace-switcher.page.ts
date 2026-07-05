import { type Element } from "./app.page.js";

// Page object for the switcher tab bar (SandboxSwitcherGroup.svelte), a DOM region of the single
// `main-navigation-panel` webview (docs/features/consolidate-app-container/02_dev.md Phase C) —
// not a separate `/workspace/switcher` webview any more.
class WorkspaceSwitcherPage {
  // Each sandbox is a `div[data-sandbox]` group carrying `button[data-app]` per open pane
  // ("dashboard" for the home tab, which always renders) and, per non-home tab, a sibling
  // `button[data-close-tab]`.
  sandboxGroup(sandbox: string): Element {
    return $(`div[data-sandbox="${sandbox}"]`);
  }

  tab(sandbox: string, kind: "dashboard" | "terminal" | "agentweb"): Element {
    return this.sandboxGroup(sandbox).$(`button[data-app="${kind}"]`);
  }

  closeTabButton(sandbox: string, kind: "terminal" | "agentweb"): Element {
    return this.sandboxGroup(sandbox).$(`button[data-close-tab="${kind}"]`);
  }

  closeSandboxButton(sandbox: string): Element {
    return this.sandboxGroup(sandbox).$("button[data-close-sandbox]");
  }

  async clickTab(sandbox: string, kind: "dashboard" | "terminal" | "agentweb"): Promise<void> {
    const tab = await this.tab(sandbox, kind);
    await tab.waitForExist({ timeout: 15000 });
    await tab.scrollIntoView();
    await tab.click();
  }

  async clickCloseTab(sandbox: string, kind: "terminal" | "agentweb"): Promise<void> {
    const button = await this.closeTabButton(sandbox, kind);
    await button.waitForExist({ timeout: 15000 });
    await button.scrollIntoView();
    await button.click();
  }
}

export default new WorkspaceSwitcherPage();
