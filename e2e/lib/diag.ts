import fs from "fs";
import path from "path";

/**
 * Registers an `afterEach` dumping the webview URL, page source and a screenshot to e2e/.diag when
 * a test fails. `extra` adds a sidecar artifact beside that dump, isolated so it cannot mask it.
 */
export function captureDiagOnFailure(
  extra?: (diagDir: string, stamp: string) => void | Promise<void>,
): void {
  // Non-arrow so Mocha keeps `this.currentTest` bound.
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
