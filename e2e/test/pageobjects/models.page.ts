import AppPage, { type Element } from "./app.page.js";
import { COMMAND, SUBPROCESS } from "../../lib/timeouts.js";

// The /models screen: probes host specs, downloads a GGUF, starts/stops the host inference server,
// and exposes a live base_url. Selectors are data-attribute based (set in
// src/routes/models/+page.svelte) so the page object never depends on CSS classes or copy.
class ModelsPage extends AppPage {
  get navLink(): Element {
    return $("[role='tab'][data-value='/models']");
  }

  get specsPanel(): Element {
    return $("[data-specs]");
  }

  // Present only once detect_machine_specs has resolved and the host numbers render (replacing the
  // "Detecting…" placeholder). Waited on in open() so callers never read the panel mid-probe.
  get specsReady(): Element {
    return $("[data-specs-ready]");
  }

  get runningPanel(): Element {
    return $("[data-running]");
  }

  get baseUrlEl(): Element {
    return $("[data-base-url]");
  }

  // The chat-test surface; only rendered while a server is running (status.running).
  get chatPanel(): Element {
    return $("[data-chat]");
  }

  // The manual "re-detect hardware" icon button in the specs section header. Only rendered once
  // specs is non-null (see specsReady), so callers must open() first.
  get redetectButton(): Element {
    return $("[data-redetect]");
  }

  async redetect(): Promise<void> {
    const btn = await this.redetectButton;
    await btn.waitForClickable({ timeout: COMMAND });
    await btn.click();
  }

  // The row for a catalogue/test model, keyed by its stable id (data-model-id).
  row(modelId: string): Element {
    return $(`[data-model-id='${modelId}']`);
  }

  // Every rendered catalogue row id (data-model-id), in display order.
  async modelIds(): Promise<string[]> {
    const rows = await $$("[data-model-id]");
    const ids: string[] = [];
    for (const r of rows) ids.push((await r.getAttribute("data-model-id")) ?? "");
    return ids;
  }

  // The rating badge text for a model row (e.g. "Recommended"). Asserts recommend.ts ran.
  async rating(modelId: string): Promise<string> {
    return (await (await this.row(modelId)).$("[data-rating]")).getText();
  }

  // Whether a model row's Start button is enabled. Used to assert the not-downloaded gate.
  async startEnabled(modelId: string): Promise<boolean> {
    return (await (await this.row(modelId)).$("[data-start]")).isEnabled();
  }

  // Whether the running-state UI is present (status panel / chat surface).
  async isRunningShown(): Promise<boolean> {
    return (await this.runningPanel).isExisting();
  }
  async isChatShown(): Promise<boolean> {
    return (await this.chatPanel).isExisting();
  }

  // Navigate to /models and wait for the host-specs probe to resolve. detect_machine_specs
  // shells out (PowerShell/winget) so the populated panel can lag the container by seconds — wait
  // for [data-specs-ready], not just the [data-specs] placeholder, or callers race the probe.
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

  // The rendered host-specs block; asserts the probe ran.
  async specsText(): Promise<string> {
    return (await this.specsPanel).getText();
  }

  async startDownload(modelId: string): Promise<void> {
    const row = await this.row(modelId);
    const btn = await row.$("button=Download");
    await btn.waitForDisplayed({ timeout: COMMAND });
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
    await btn.waitForClickable({ timeout: COMMAND });
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

  // Reads the running endpoint's base_url. Waits for the element: it only renders inside the
  // running model row ([data-running]), so a missing element means the row isn't running yet —
  // surfacing as a timeout here rather than a bare "element not found" at the call site.
  async baseUrl(): Promise<string> {
    const el = await this.baseUrlEl;
    await el.waitForDisplayed({ timeout: COMMAND });
    return el.getText();
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
    await btn.waitForClickable({ timeout: COMMAND });
    await btn.click();
    // stop_inference kills the llama child synchronously (fast) then drops the handle, but tears the
    // reverse Mutagen forward down on a detached task (lib.rs) — `forward terminate` can take many
    // seconds against the VM listener and must not block the command. So the command returns
    // promptly, inference_status reports not-running, and [data-running] clears on the next poll. A
    // timeout here means the command itself errored or never returned, not a slow forward teardown.
    await browser.waitUntil(async () => !(await this.runningPanel).isExisting(), {
      timeout: SUBPROCESS,
      interval: 500,
      timeoutMsg: `status panel still shows running ${SUBPROCESS}ms after clicking Stop — stop_inference ` +
        "did not drop the server handle (the command errored or never returned).",
    });
  }
}

export default new ModelsPage();
