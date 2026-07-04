import { type Element } from "./app.page.js";

class WorkspaceSwitcherPage {
  // Poll every window handle until one is the switcher route (mirrors switchToDashboard in
  // workspace-dashboard.page.ts). The switcher webview exists from app startup, but polling
  // tolerates slow handle registration under tauri-driver.
  async switchToSwitcher(timeoutMs = 15000): Promise<void> {
    await browser.waitUntil(
      async () => {
        for (const handle of await browser.getWindowHandles()) {
          await browser.switchToWindow(handle);
          const pagePath = await browser.execute(() => window.location.pathname);
          if (pagePath === "/workspace/switcher") return true;
        }
        return false;
      },
      {
        timeout: timeoutMs,
        interval: 500,
        timeoutMsg: "the /workspace/switcher webview never appeared among the window handles",
      },
    );
  }

  async switchToAppWebview(): Promise<void> {
    for (const handle of await browser.getWindowHandles()) {
      await browser.switchToWindow(handle);
      const pagePath = await browser.execute(() => window.location.pathname);
      if (!pagePath.startsWith("/workspace")) return;
    }
    throw new Error("[e2e] no app webview found among the window handles");
  }

  // Each sandbox is a `div[data-sandbox]` group carrying `button[data-app]` per open pane
  // ("dashboard" for the home tab) and, per non-home tab, a sibling `button[data-close-tab]`.
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
