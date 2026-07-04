import fs from "fs";
import path from "path";

// Registers an `afterEach` that, only when the current test failed, dumps the webview URL + page
// source + a screenshot to e2e/.diag (files stamped from the test title). This exact block was
// copy-pasted verbatim across several standard/heavy specs before this helper existed;
// useDashboardWebview() (lib/dashboard-suite.ts) and the standalone specs all call it now.
//
// `extra` runs after the page dump for specs that also want a sidecar artifact (e.g. local-llm
// copies llama-server.log). It receives the same diag dir + stamp so its files land beside the page
// dump, and runs inside its own try/catch so a failing extra step never masks the page dump.
//
// Non-arrow `afterEach(async function () { ... })` so `this.currentTest` stays bound — matching
// every original copy.
export function captureDiagOnFailure(
  extra?: (diagDir: string, stamp: string) => void | Promise<void>,
): void {
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
      console.log(`[diag] url=${url} — dumped ${stamp}.html/.png to ${diag}`);
    } catch (e) {
      console.log(`[diag] could not capture page state: ${String(e)}`);
    }
    try {
      await extra?.(diag, stamp);
    } catch (e) {
      console.log(`[diag] could not capture extra artifact: ${String(e)}`);
    }
  });
}
