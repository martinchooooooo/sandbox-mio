import AppPage, { type Element } from "./app.page.js";
import { COMMAND, SUBPROCESS } from "../../lib/timeouts.js";

// Selectors are data-attribute based (set in src/lib/models/) so this page object
// never depends on CSS classes or copy.
class ModelsPage extends AppPage {
  get specsPanel(): Element {
    return $("[data-specs]");
  }

  // The hardware list sits behind the computer card's "For those who know" disclosure, closed by
  // default on every page visit, so open() expands it.
  get computerTrigger(): Element {
    return $("[data-computer-details] button");
  }

  async #expand(trigger: Element, timeout: number): Promise<void> {
    const btn = await trigger;
    await btn.waitForDisplayed({ timeout });
    if ((await btn.getAttribute("data-state")) === "open") return;
    await btn.waitForClickable({ timeout });
    await btn.click();
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

  get wakeDetailsTrigger(): Element {
    return $("[data-wake-details] button");
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

  /** Each card's id, fit verdict and summary, and the fit group it sits under (null for Your Model). */
  async cards(): Promise<{ id: string; fit: string; group: string | null; summary: string; canStart: boolean }[]> {
    return browser.execute(() =>
      Array.from(document.querySelectorAll("[data-model-id]")).map((el) => ({
        id: el.getAttribute("data-model-id") ?? "",
        fit: el.getAttribute("data-fit") ?? "",
        group: el.closest("[data-fit-group]")?.getAttribute("data-fit-group") ?? null,
        summary: el.querySelector("[data-summary]")?.textContent?.trim() ?? "",
        canStart: el.querySelector("[data-start]") !== null,
      })),
    );
  }

  /** The fit label (e.g. "Runs well here"); its presence proves recommend.ts ran. */
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
    await this.showPage("/models", timeout);
    await (await this.specsPanel).waitForDisplayed({ timeout: SUBPROCESS });
    await this.#expand(this.computerTrigger, SUBPROCESS);
    await (await this.specsReady).waitForDisplayed({ timeout: SUBPROCESS });
  }

  async specsText(): Promise<string> {
    return (await this.specsPanel).getText();
  }

  /**
   * The Your Model card's single button chains download and wake; any other card downloads through
   * "Use this Model" (also `[data-start]`), so callers need not know which card a model is on.
   */
  async startDownload(modelId: string): Promise<void> {
    const btn = await (await this.row(modelId)).$("[data-start]");
    await btn.waitForClickable({ timeout: COMMAND });
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

  /**
   * No-ops if `[data-start]` is already gone: the Your Model card's combined `startDownload()` click
   * already triggered start too, in which case the button switched to Sleep (no `data-start`).
   */
  async startInference(modelId: string): Promise<void> {
    const row = await this.row(modelId);
    const btn = row.$("[data-start]");
    if (!(await btn.isExisting())) return;
    const el = await btn;
    await el.waitForClickable({ timeout: COMMAND });
    await el.click();
  }

  async waitForRunning(timeoutMs: number): Promise<void> {
    await browser.waitUntil(async () => (await this.runningPanel).isExisting(), {
      timeout: timeoutMs,
      interval: 2000,
      timeoutMsg: `inference server did not reach running state within ${timeoutMs}ms`,
    });
  }

  // Waits rather than reads: the attribute only exists while the server runs, so a not-yet-running
  // server surfaces as a timeout here instead of "element not found" at the call site.
  async baseUrl(): Promise<string> {
    const el = await this.baseUrlEl;
    await el.waitForExist({ timeout: COMMAND });
    return (await el.getAttribute("data-base-url")) ?? "";
  }

  async stepTitles(): Promise<string[]> {
    await this.#expand(this.wakeDetailsTrigger, COMMAND);
    const titles = await $$("[data-step-id] [data-step-title]");
    const texts: string[] = [];
    for (const t of titles) texts.push((await t.getText()).trim());
    return texts;
  }

  async stopInference(): Promise<void> {
    const btn = await (await this.runningPanel).$("[data-stop]");
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
