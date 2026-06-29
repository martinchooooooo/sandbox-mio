import AppPage, { type Element } from "./app.page.js";

// The /settings screen: picks the inference engine, persists advanced llama options, and renders
// the static runtime-setup explainer (the same RuntimeSteps component the /models page drives
// live). Selectors are data-attribute based (set in src/routes/settings/+page.svelte and
// src/lib/models/RuntimeSteps.svelte) so the page object never depends on CSS classes or copy.
class SettingsPage extends AppPage {
  get navLink(): Element {
    return $("a[href='/settings']");
  }

  // Wraps the engine buttons; its data-active-engine reflects the currently selected engine.
  get engineGroup(): Element {
    return $("[data-active-engine]");
  }

  get saveButton(): Element {
    return $("[data-save]");
  }

  get savedMarker(): Element {
    return $("[data-saved]");
  }

  // Navigate to /settings and wait for the engine group to render. The nav link only exists on the
  // home page, so callers must start from home (the app launches there).
  async open(timeout = 10000): Promise<void> {
    const url = await browser.getUrl();
    if (!url.includes("/settings")) {
      const link = await this.navLink;
      await link.waitForDisplayed({ timeout });
      await link.click();
    }
    await (await this.engineGroup).waitForDisplayed({ timeout });
  }

  // The engine currently shown as selected ("llama" | "foundry").
  async activeEngine(): Promise<string> {
    return (await (await this.engineGroup).getAttribute("data-active-engine")) ?? "";
  }

  async selectEngine(engine: "llama" | "foundry"): Promise<void> {
    const btn = await $(`[data-engine='${engine}']`);
    await btn.waitForClickable({ timeout: 10000 });
    await btn.click();
  }

  // The non-empty titles of every rendered runtime-setup checklist row (static explainer mode).
  async stepTitles(): Promise<string[]> {
    const titles = await $$("[data-step-id] [data-step-title]");
    const texts: string[] = [];
    for (const t of titles) texts.push((await t.getText()).trim());
    return texts;
  }

  // Save the current settings and wait for the saved confirmation to appear.
  async save(): Promise<void> {
    const btn = await this.saveButton;
    await btn.waitForClickable({ timeout: 10000 });
    await btn.click();
    await (await this.savedMarker).waitForDisplayed({ timeout: 10000 });
  }
}

export default new SettingsPage();
