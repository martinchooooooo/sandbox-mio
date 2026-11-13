import AppPage, { navButton, type Element } from "./app.page.js";
import { COMMAND } from "../../lib/timeouts.js";

// Selectors are data-attribute based (src/routes/settings/+page.svelte,
// src/lib/models/RuntimeSteps.svelte) so this page object never depends on CSS classes or copy.
class SettingsPage extends AppPage {
  get navLink(): Element {
    return navButton("/settings");
  }

  get engineGroup(): Element {
    return $("[data-active-engine]");
  }

  // The engine/provider picker and the reset-machine button sit behind two separate
  // AdvancedSection disclosures, closed by default on every page visit
  // (docs/features/desktop-app-feel/02_dev.md Phase D) — `data-advanced-section` scopes each
  // instance's own root so its trigger button can be found without depending on copy.
  get tuningTrigger(): Element {
    return $("[data-advanced-section='tuning'] button");
  }

  get troubleshootingTrigger(): Element {
    return $("[data-advanced-section='troubleshooting'] button");
  }

  async #expand(trigger: Element): Promise<void> {
    const btn = await trigger;
    if ((await btn.getAttribute("data-state")) === "open") return;
    await btn.waitForClickable({ timeout: COMMAND });
    await btn.click();
  }

  get saveButton(): Element {
    return $("[data-save]");
  }

  get savedMarker(): Element {
    return $("[data-saved]");
  }

  get resetPodmanMachineButton(): Element {
    return $("[data-reset-podman-machine]");
  }

  get resetPodmanMachineDoneMarker(): Element {
    return $("[data-reset-podman-machine-done]");
  }

  /**
   * Waits on the "tuning" AdvancedSection's trigger rather than `engineGroup`: the latter is inside
   * its (closed-by-default) content, so it would never become displayed on a fresh page visit.
   */
  async open(timeout = COMMAND): Promise<void> {
    await this.showPage("/settings", timeout);
    await (await this.tuningTrigger).waitForDisplayed({ timeout });
  }

  async activeEngine(): Promise<string> {
    await this.#expand(this.tuningTrigger);
    return (await (await this.engineGroup).getAttribute("data-active-engine")) ?? "";
  }

  async selectEngine(engine: "llama" | "foundry"): Promise<void> {
    await this.#expand(this.tuningTrigger);
    const btn = await $(`[data-engine='${engine}']`);
    await btn.waitForClickable({ timeout: COMMAND });
    await btn.click();
  }

  async stepTitles(): Promise<string[]> {
    const titles = await $$("[data-step-id] [data-step-title]");
    const texts: string[] = [];
    for (const t of titles) texts.push((await t.getText()).trim());
    return texts;
  }

  async save(): Promise<void> {
    const btn = await this.saveButton;
    await btn.waitForClickable({ timeout: COMMAND });
    await btn.click();
    await (await this.savedMarker).waitForDisplayed({ timeout: COMMAND });
  }

  async resetPodmanMachine(): Promise<void> {
    await this.#expand(this.troubleshootingTrigger);
    const btn = await this.resetPodmanMachineButton;
    await btn.waitForClickable({ timeout: COMMAND });
    await btn.click();
    await (await this.resetPodmanMachineDoneMarker).waitForDisplayed({ timeout: COMMAND });
  }
}

export default new SettingsPage();
