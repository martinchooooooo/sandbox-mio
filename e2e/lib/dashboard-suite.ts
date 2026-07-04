import fs from "fs";
import path from "path";

import workspaceDashboard from "../test/pageobjects/workspace-dashboard.page.js";

// Every spec whose tests operate on a sandbox's dashboard webview (package-autocomplete,
// expose-port, agent-setup, runtime-management, opencode-agent) copy-pasted the same pair of
// afterEach hooks: leave the session on the app webview between tests - the global `before`
// hook (e2e/wdio.conf.ts) only pins the session there once per session, and tests that don't touch
// the dashboard (navigateHome-based reloads, row.deployTo, row.destroy) assume that default - and
// dump page state to e2e/.diag on failure. Call this once per `describe` block.
export function useDashboardWebview(): void {
  afterEach(async () => {
    await workspaceDashboard.switchToAppWebview().catch(() => {});
  });

  afterEach(async function () {
    if (this.currentTest?.state !== "failed") return;
    const diag = path.join(process.cwd(), ".diag");
    fs.mkdirSync(diag, { recursive: true });
    const stamp = (this.currentTest?.title ?? "test").replace(/[^a-z0-9]+/gi, "-").slice(0, 40);
    try {
      const url = await browser.getUrl();
      const src = await browser.getPageSource();
      fs.writeFileSync(path.join(diag, `${stamp}.html`), `<!-- url: ${url} -->\n${src}`);
      await browser.saveScreenshot(path.join(diag, `${stamp}.png`));
      console.log(`[diag] url=${url} - dumped ${stamp}.html/.png to ${diag}`);
    } catch (e) {
      console.log(`[diag] could not capture page state: ${String(e)}`);
    }
  });
}

// Wraps a test body that needs sandbox `sandbox()`'s dashboard webview: switches to it before
// running `fn`. The afterEach above resets every test back to the app webview, so any test that
// touches dashboard elements has to re-attach - that manual step is exactly what
// package-autocomplete.spec.ts forgot for four tests in a row, and the failure it produced
// ("element wasn't found") pointed nowhere near the actual cause. Baking the switch into the
// wrapper means a test written with this helper cannot forget it.
//
// Do not use this for a suite's first dashboard-opening test: the pane does not exist until that
// test creates/opens the workspace, so it calls workspaceDashboard.switchToDashboard itself after
// doing so. Also skip it for tests that must start on the app webview (e.g. a navigateHome()
// reload) even though they switch to the dashboard partway through their own body.
export function dashboardIt(title: string, sandbox: () => string, fn: () => Promise<void>): void {
  it(title, async () => {
    await workspaceDashboard.switchToDashboard(sandbox());
    await fn();
  });
}
