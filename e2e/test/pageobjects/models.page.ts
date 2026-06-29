import AppPage, { type Element } from "./app.page.js";

// The /models screen: probes host specs, downloads a GGUF, starts/stops the host inference server,
// and exposes a live base_url. Selectors are data-attribute based (set in
// src/routes/models/+page.svelte) so the page object never depends on CSS classes or copy.
class ModelsPage extends AppPage {
  get navLink(): Element {
    return $("a[href='/models']");
  }

  get specsPanel(): Element {
    return $("[data-specs]");
  }

  get runningPanel(): Element {
    return $("[data-running]");
  }

  get baseUrlEl(): Element {
    return $("[data-base-url]");
  }

  // The row for a catalogue/test model, keyed by its stable id (data-model-id).
  row(modelId: string): Element {
    return $(`[data-model-id='${modelId}']`);
  }

  // Navigate to /models and wait for the specs panel to render. The nav link only exists on the
  // home page, so callers must start from home (the app launches there).
  async open(timeout = 10000): Promise<void> {
    const url = await browser.getUrl();
    if (!url.includes("/models")) {
      const link = await this.navLink;
      await link.waitForDisplayed({ timeout });
      await link.click();
    }
    await (await this.specsPanel).waitForDisplayed({ timeout });
  }

  // The rendered host-specs block; asserts the probe ran.
  async specsText(): Promise<string> {
    return (await this.specsPanel).getText();
  }

  async startDownload(modelId: string): Promise<void> {
    const row = await this.row(modelId);
    const btn = await row.$("button=Download");
    await btn.waitForDisplayed({ timeout: 10000 });
    await btn.click();
  }

  // Poll until the model's row shows the downloaded marker.
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
    await btn.waitForClickable({ timeout: 10000 });
    await btn.click();
  }

  // Poll until the status panel reports a running server with a base_url.
  async waitForRunning(timeoutMs: number): Promise<void> {
    await browser.waitUntil(async () => (await this.runningPanel).isExisting(), {
      timeout: timeoutMs,
      interval: 2000,
      timeoutMsg: `inference server did not reach running state within ${timeoutMs}ms`,
    });
  }

  async baseUrl(): Promise<string> {
    return (await this.baseUrlEl).getText();
  }

  // The non-empty titles of every rendered runtime-setup checklist row.
  async stepTitles(): Promise<string[]> {
    const titles = await $$("[data-step-id] [data-step-title]");
    const texts: string[] = [];
    for (const t of titles) texts.push((await t.getText()).trim());
    return texts;
  }

  async stopInference(): Promise<void> {
    const btn = await (await this.runningPanel).$("button=Stop");
    await btn.waitForClickable({ timeout: 10000 });
    await btn.click();
    await browser.waitUntil(async () => !(await this.runningPanel).isExisting(), {
      timeout: 30000,
      interval: 1000,
      timeoutMsg: "status panel still shows running after stop",
    });
  }
}

export default new ModelsPage();
