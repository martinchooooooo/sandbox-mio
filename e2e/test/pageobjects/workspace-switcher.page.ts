import { type Element } from "./app.page.js";
import { PANEL_RENDER } from "../../lib/timeouts.js";

// The switcher tab bar (SandboxSwitcherGroup.svelte) is a DOM region of `main-navigation-panel`,
// not a webview of its own (docs/features/consolidate-app-container/02_dev.md Phase C).
class WorkspaceSwitcherPage {
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
    await tab.waitForExist({ timeout: PANEL_RENDER });
    await tab.scrollIntoView();
    await tab.click();
  }

  async clickCloseTab(sandbox: string, kind: "terminal" | "agentweb"): Promise<void> {
    const button = await this.closeTabButton(sandbox, kind);
    await button.waitForExist({ timeout: PANEL_RENDER });
    await button.scrollIntoView();
    await button.click();
  }
}

export default new WorkspaceSwitcherPage();
