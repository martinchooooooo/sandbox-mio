import { type Element } from "./app.page.js";
import { AGENT_INSTALL, COMMAND, PACKAGE_INSTALL, PANEL_RENDER, SUBPROCESS } from "../../lib/timeouts.js";

// A DOM region of the single `main-navigation-panel` webview, not a separate one — no window
// switching needed (docs/features/consolidate-app-container/02_dev.md Phase C).
type DashboardSectionId = "overview" | "tools" | "ports" | "network" | "traffic" | "remote" | "danger";

class WorkspaceDashboardPage {
  // `[data-section-nav]` renders for every section once WorkspaceDashboard.svelte's `workspacePath`
  // resolves; the package input exists only in the Tools section. `sandbox` only names the error.
  async waitForRendered(sandbox: string): Promise<void> {
    await this.sectionNav.waitForExist({
      timeout: PANEL_RENDER,
      timeoutMsg: `dashboard content for "${sandbox}" never rendered (workspacePath likely never resolved)`,
    });
  }

  get sectionNav(): Element {
    return $("[data-section-nav]");
  }

  sectionItem(id: DashboardSectionId): Element {
    return $(`[data-section-nav-item="${id}"]`);
  }

  /** Switches to a section (a no-op when it is already active) and waits until it is active. */
  async openSection(id: DashboardSectionId): Promise<void> {
    const item = this.sectionItem(id);
    await item.waitForExist({ timeout: PANEL_RENDER, timeoutMsg: `section "${id}" is not in the section list` });
    if ((await item.getAttribute("data-state")) !== "active") {
      await item.scrollIntoView();
      await item.click();
    }
    await browser.waitUntil(async () => (await item.getAttribute("data-state")) === "active", {
      timeout: COMMAND,
      timeoutMsg: `section "${id}" did not become active`,
    });
  }

  // `data-dashboard` (WorkspaceDashboard.svelte's `<main>`) scopes the getters below: the sandbox
  // list (SandboxItem.svelte) and the Dashboard render as siblings, both simultaneously visible
  // (docs/features/consolidate-app-container/02_dev.md), and both use StatusRow — an unscoped
  // `errorRow` lookup could resolve to the list row's instead of the Dashboard's.
  get root(): Element {
    return $("[data-dashboard]");
  }

  // ── App launcher (docs/features/workspace-app-tabs/02_dev.md Phase D) ──────

  launcherButton(kind: "terminal" | "agentweb"): Element {
    return $(`button[data-app="${kind}"]`);
  }

  async launchApp(kind: "terminal" | "agentweb"): Promise<void> {
    await this.#clickButton(this.launcherButton(kind));
  }

  // ── Package management ─────────────────────────────────────────────────────

  get packageInput(): Element {
    return $("input[placeholder='package name']");
  }

  get installButton(): Element {
    return $("button=install");
  }

  async #clickButton(button: Element): Promise<void> {
    const el = await button;
    await el.scrollIntoView();
    await el.click();
  }

  // One click is not enough: a click landing before WebView2 has routed input focus reports success
  // yet leaves the element unfocused, so the `onfocus` handler that fetches suggestions never fires.
  async #focusInput(input: Element): Promise<void> {
    const el = await input;
    await el.scrollIntoView();

    await browser.waitUntil(
      async () => {
        await el.click();
        return browser.execute((node) => document.activeElement === node, el);
      },
      {
        timeout: COMMAND,
        interval: 300,
        timeoutMsg: "input never gained document focus after switching to the dashboard webview",
      },
    );
  }

  async installPackage(pkg: string, { timeout = PACKAGE_INSTALL }: { timeout?: number } = {}): Promise<void> {
    await this.#focusInput(this.packageInput);
    await (await this.packageInput).setValue(pkg);
    await this.#clickButton(this.installButton);
    await this.waitForPackage(pkg, timeout);
  }

  async installedPackages(): Promise<string[]> {
    // Scoped to the active section: other panels (proxy rules, flows) render monospace spans too.
    const chips = await $$('[data-slot="section-nav-content"][data-state="active"] span.font-mono');
    const names: string[] = [];
    for (const chip of chips) names.push(await chip.getText());
    return names;
  }

  async waitForPackage(pkg: string, timeoutMs = PACKAGE_INSTALL): Promise<void> {
    await browser.waitUntil(async () => (await this.installedPackages()).includes(pkg), {
      timeout: timeoutMs,
      interval: 2000,
      timeoutMsg: `package "${pkg}" did not appear in the dashboard within ${timeoutMs}ms`,
    });
  }

  // ── Package autocomplete ────────────────────────────────────────────────────
  // Selectors are body-rooted, not input-rooted: the dropdown is portalled to document.body.

  async typeInPackageInput(text: string): Promise<void> {
    await this.#focusInput(this.packageInput);
    await (await this.packageInput).setValue(text);
  }

  async visibleSuggestions(): Promise<string[]> {
    const items = await $$(".fixed.bg-popover button[type='button']");
    const names: string[] = [];
    for (const item of items) {
      const span = await item.$("span.font-mono");
      if (await span.isExisting()) names.push(await span.getText());
    }
    return names;
  }

  async suggestionDescription(shortName: string): Promise<string> {
    const items = await $$(".fixed.bg-popover button[type='button']");
    for (const item of items) {
      const nameSpan = await item.$("span.font-mono");
      if (!(await nameSpan.isExisting())) continue;
      if ((await nameSpan.getText()) !== shortName) continue;
      const desc = await item.$("span.text-muted-foreground");
      return (await desc.isExisting()) ? desc.getText() : "";
    }
    return "";
  }

  async selectSuggestion(shortName: string): Promise<void> {
    const items = await $$(".fixed.bg-popover button[type='button']");
    for (const item of items) {
      const span = await item.$("span.font-mono");
      if ((await span.isExisting()) && (await span.getText()) === shortName) {
        await item.scrollIntoView();
        await item.click();
        return;
      }
    }
    throw new Error(`suggestion "${shortName}" not found in the dropdown`);
  }

  async packageInputValue(): Promise<string> {
    return (await this.packageInput).getValue();
  }

  // ── Agent management ────────────────────────────────────────────────────────

  get addAgentButton(): Element {
    return $("button=Add agent");
  }

  get noAgentsText(): Element {
    return $("span=No agents installed.");
  }

  agentInstallButton(agentLabel: string): Element {
    return $(`span*=${agentLabel}`).$("../..").$("button=install");
  }

  agentAuthButton(agentLabel: string): Element {
    return $(`span=${agentLabel}`).$("..").$("button*=thenticate");
  }

  agentStatusText(agentLabel: string): Element {
    return $(`span=${agentLabel}`).$("..").$("span.text-muted-foreground, span.text-amber-600");
  }

  agentProviderHint(agentLabel: string): Element {
    return $(`span=${agentLabel}`).$("..").$("a[href='/models']");
  }

  async startAgentInstall(agentLabel: string): Promise<void> {
    await this.#clickButton(this.addAgentButton);
    await this.#clickButton(this.agentInstallButton(agentLabel));
  }

  async waitForAgentRow(agentLabel: string, timeoutMs = AGENT_INSTALL): Promise<void> {
    await browser.waitUntil(async () => (await $(`span=${agentLabel}`)).isExisting(), {
      timeout: timeoutMs,
      interval: 3000,
      timeoutMsg: `agent row for "${agentLabel}" did not appear`,
    });
  }

  async waitForAgentStatus(agentLabel: string, text: string, timeoutMs = COMMAND): Promise<void> {
    await browser.waitUntil(
      async () => {
        try {
          return (await this.agentStatusText(agentLabel).getText()) === text;
        } catch {
          return false;
        }
      },
      { timeout: timeoutMs, interval: 500, timeoutMsg: `agent status for "${agentLabel}" did not become "${text}"` },
    );
  }

  // ── Port management (PortsManager.svelte) ──────────────────────────────────
  // `kindLabel` keys a row: "Exposed :4000" for a manually-exposed port, or the `.mio/ports.json`
  // label (e.g. "webapp") for a file-declared one.

  get exposePortInput(): Element {
    return $("input[placeholder='Expose a port (e.g. 3000)']");
  }

  get exposeButton(): Element {
    return $("button=Expose");
  }

  get noPortsText(): Element {
    return $("span=No ports open.");
  }

  portRow(kindLabel: string): Element {
    return $(`span=${kindLabel}`).$("..");
  }

  // portRow() chains `.$("..")` onto the label span, which throws when missing instead of
  // returning a not-existing element — that would abort waitUntil's poll instead of retrying.
  #portLabelSpan(kindLabel: string): Element {
    return $(`span=${kindLabel}`);
  }

  async exposePort(port: number): Promise<void> {
    const input = await this.exposePortInput;
    await input.scrollIntoView();
    await input.setValue(String(port));
    await this.#clickButton(this.exposeButton);
  }

  async waitForPortRow(kindLabel: string, timeoutMs = SUBPROCESS): Promise<void> {
    await browser.waitUntil(async () => (await this.#portLabelSpan(kindLabel)).isExisting(), {
      timeout: timeoutMs,
      interval: 500,
      timeoutMsg: `port row "${kindLabel}" did not appear`,
    });
  }

  async waitForPortRowGone(kindLabel: string, timeoutMs = SUBPROCESS): Promise<void> {
    await browser.waitUntil(async () => !(await this.#portLabelSpan(kindLabel).isExisting()), {
      timeout: timeoutMs,
      interval: 500,
      timeoutMsg: `port row "${kindLabel}" is still displayed`,
    });
  }

  async portRowHostPort(kindLabel: string): Promise<number> {
    const row = await this.portRow(kindLabel);
    const link = await row.$("button[data-port-url]");
    const text = await link.getText();
    const match = text.match(/:(\d+)\/?$/);
    if (!match) throw new Error(`port row "${kindLabel}" button text "${text}" has no port`);
    return Number(match[1]);
  }

  /** Only manually-exposed rows carry one; file-declared ports cannot be closed from the UI. */
  async hasCloseButton(kindLabel: string): Promise<boolean> {
    const row = await this.portRow(kindLabel);
    return (await row.$("button[title='Stop exposing']")).isExisting();
  }

  async closePortRow(kindLabel: string): Promise<void> {
    const row = await this.portRow(kindLabel);
    await this.#clickButton(row.$("button[title='Stop exposing']"));
  }

  // ── Folder reveal / Destroy (WorkspaceDashboard.svelte) ─────────────────────

  get folderButton(): Element {
    return this.root.$("button=folder");
  }

  get destroyButton(): Element {
    return this.root.$("button=destroy");
  }

  async destroySandbox(): Promise<void> {
    await this.#clickButton(this.destroyButton);
  }

  // ── Remote management (RemoteManager.svelte) ────────────────────────────────

  get deployButton(): Element {
    return this.root.$("button=deploy");
  }

  get deployConfirmButton(): Element {
    return this.root.$("button=Deploy");
  }

  get undeployButton(): Element {
    return this.root.$("button=undeploy");
  }

  get hostSelect(): Element {
    return this.root.$("select");
  }

  get syncSwitch(): Element {
    return this.root.$("button[role='switch']");
  }

  /**
   * StatusRow's error line for the Dashboard's shared action runner — deploy/undeploy/destroy/sync
   * and folder-reveal all run through it (`action` in WorkspaceDashboard.svelte), so one selector
   * covers all of them, scoped to `root` (see the comment on it above).
   */
  get errorRow(): Element {
    return this.root.$("div[role='status'].text-destructive");
  }
}

export default new WorkspaceDashboardPage();
