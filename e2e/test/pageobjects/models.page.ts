import AppPage, { type Element } from "./app.page.js";
import { COMMAND, SUBPROCESS } from "../../lib/timeouts.js";

// Selectors are data-attribute based (set in src/routes/models/+page.svelte) so this page object
// never depends on CSS classes or copy.
class ModelsPage extends AppPage {
  get navLink(): Element {
    return $("[role='tab'][data-value='/models']");
  }

  get specsPanel(): Element {
    return $("[data-specs]");
  }

  // Present only once detect_machine_specs has resolved, so open() waits on it rather than the
  // specsPanel placeholder — otherwise callers read the panel mid-probe.
  get specsReady(): Element {
    return $("[data-specs-ready]");
  }

  get runningPanel(): Element {
    return $("[data-running]");
  }

  get baseUrlEl(): Element {
    return $("[data-base-url]");
  }

  // Only rendered while a server is running.
  get chatPanel(): Element {
    return $("[data-chat]");
  }

  // Only rendered once specs resolve, so callers must open() first.
  get redetectButton(): Element {
    return $("[data-redetect]");
  }

  async redetect(): Promise<void> {
    const btn = await this.redetectButton;
    await btn.waitForClickable({ timeout: COMMAND });
    await btn.click();
  }

  row(modelId: string): Element {
    return $(`[data-model-id='${modelId}']`);
  }

  /** In display order. */
  async modelIds(): Promise<string[]> {
    const rows = await $$("[data-model-id]");
    const ids: string[] = [];
    for (const r of rows) ids.push((await r.getAttribute("data-model-id")) ?? "");
    return ids;
  }

  /** The badge text (e.g. "Recommended"); its presence proves recommend.ts ran. */
  async rating(modelId: string): Promise<string> {
    return (await (await this.row(modelId)).$("[data-rating]")).getText();
  }

  async startEnabled(modelId: string): Promise<boolean> {
    return (await (await this.row(modelId)).$("[data-start]")).isEnabled();
  }

  async isRunningShown(): Promise<boolean> {
    return (await this.runningPanel).isExisting();
  }
  async isChatShown(): Promise<boolean> {
    return (await this.chatPanel).isExisting();
  }

  /** Returns only once the host-specs probe has resolved; it shells out and can lag by seconds. */
  async open(timeout = COMMAND): Promise<void> {
    const url = await browser.getUrl();
    if (!url.includes("/models")) {
      const link = await this.navLink;
      await link.waitForDisplayed({ timeout });
      await link.click();
    }
    await (await this.specsPanel).waitForDisplayed({ timeout });
    await (await this.specsReady).waitForDisplayed({ timeout: SUBPROCESS });
  }

  async specsText(): Promise<string> {
    return (await this.specsPanel).getText();
  }

  async startDownload(modelId: string): Promise<void> {
    const row = await this.row(modelId);
    const btn = await row.$("button=Download");
    await btn.waitForDisplayed({ timeout: COMMAND });
    await btn.click();
  }

  async waitForDownloaded(modelId: string, timeoutMs: number): Promise<void> {
    const row = await this.row(modelId);
    await browser.waitUntil(async () => (await row.$("[data-downloaded]")).isExisting(), {
      timeout: timeoutMs,
      interval: 2000,
      timeoutMsg: `model ${modelId} did not reach a downloaded state within ${timeoutMs}ms`,
    });
  }

  async startInference(modelId: string): Promise<void> {
    const row = await this.row(modelId);
    const btn = await row.$("[data-start]");
    await btn.waitForClickable({ timeout: COMMAND });
    await btn.click();
  }

  async waitForRunning(timeoutMs: number): Promise<void> {
    await browser.waitUntil(async () => (await this.runningPanel).isExisting(), {
      timeout: timeoutMs,
      interval: 2000,
      timeoutMsg: `inference server did not reach running state within ${timeoutMs}ms`,
    });
  }

  // Waits rather than reads: the element only renders inside a running row, so a not-yet-running
  // server surfaces as a timeout here instead of "element not found" at the call site.
  async baseUrl(): Promise<string> {
    const el = await this.baseUrlEl;
    await el.waitForDisplayed({ timeout: COMMAND });
    return el.getText();
  }

  async stepTitles(): Promise<string[]> {
    const titles = await $$("[data-step-id] [data-step-title]");
    const texts: string[] = [];
    for (const t of titles) texts.push((await t.getText()).trim());
    return texts;
  }

  async stopInference(): Promise<void> {
    const btn = await (await this.runningPanel).$("button=Stop");
    await btn.waitForClickable({ timeout: COMMAND });
    await btn.click();
    // stop_inference drops the server handle synchronously and tears the Mutagen forward down on a
    // detached task, so a timeout here means the command errored, never a slow forward teardown.
    await browser.waitUntil(async () => !(await this.runningPanel).isExisting(), {
      timeout: SUBPROCESS,
      interval: 500,
      timeoutMsg: `status panel still shows running ${SUBPROCESS}ms after clicking Stop — stop_inference ` +
        "did not drop the server handle (the command errored or never returned).",
    });
  }
}

export default new ModelsPage();
