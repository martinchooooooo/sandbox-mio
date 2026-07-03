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

  agentsTab(sandbox: string): Element {
    return $(`button*=${sandbox} — Agents`);
  }

  paneTab(sandbox: string, title: string): Element {
    return $(`button*=${sandbox} — ${title}`);
  }

  async clickAgentsTab(sandbox: string): Promise<void> {
    const tab = await this.agentsTab(sandbox);
    await tab.waitForExist({ timeout: 15000 });
    await tab.scrollIntoView();
    await tab.click();
  }
}

export default new WorkspaceSwitcherPage();
