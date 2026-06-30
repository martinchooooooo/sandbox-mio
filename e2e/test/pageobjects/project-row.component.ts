import { type Element } from "./app.page.js";

/**
 * A single project's row in the projects list. Component object: it wraps one `<li>` and
 * exposes every control inside it (start/stop, deploy/undeploy, destroy, the sync switch,
 * the status indicators) plus the waits that go with them, so specs operate a project by
 * name without knowing the row's markup.
 *
 * The row is resolved lazily on each access via `li*=<projectName>`, so the component keeps
 * working after the frontend re-renders the row in place.
 */
export default class ProjectRow {
  readonly projectName: string;

  constructor(projectName: string) {
    this.projectName = projectName;
  }

  get root(): Element {
    return $(`li*=${this.projectName}`);
  }

  get startButton(): Element {
    return this.root.$("button=start");
  }

  get stopButton(): Element {
    return this.root.$("button=stop");
  }

  get terminalButton(): Element {
    return this.root.$("button=terminal");
  }

  get deployButton(): Element {
    return this.root.$("button=deploy");
  }

  get undeployButton(): Element {
    return this.root.$("button=undeploy");
  }

  get destroyButton(): Element {
    return this.root.$("button=destroy");
  }

  get hostSelect(): Element {
    return this.root.$("select");
  }

  get syncSwitch(): Element {
    return this.root.$("button[role='switch']");
  }

  get packageInput(): Element {
    return this.root.$("input[placeholder='package name']");
  }

  get installButton(): Element {
    return this.root.$("button=install");
  }

  // The green dot shown while the container is running.
  get runningIndicator(): Element {
    return this.root.$("span.bg-green-500");
  }

  // The muted status text (e.g. "running") next to the project.
  get statusText(): Element {
    return this.root.$(".text-muted-foreground");
  }

  // The label naming the host a project is deployed to.
  deployedHostLabel(host: string): Element {
    return this.root.$(`span*=${host}`);
  }

  async waitForDisplayed(timeout = 10000): Promise<void> {
    await (await this.root).waitForDisplayed({ timeout });
  }

  async waitForGone(timeout = 15000): Promise<void> {
    await (await this.root).waitForExist({ timeout, reverse: true });
  }

  // Click a button after scrolling it into view — the row's later controls can sit off-screen.
  async #clickButton(button: Element): Promise<void> {
    const el = await button;
    await el.scrollIntoView();
    await el.click();
  }

  async start(): Promise<void> {
    await this.#clickButton(this.startButton);
  }

  async stop(): Promise<void> {
    await this.#clickButton(this.stopButton);
  }

  // Clicks the "Terminal" button, invoking the open_terminal command. The terminal opens in a
  // separate top-level WebviewWindow that tauri-driver cannot drive; the spec asserts the
  // command's side effects (token file, server online, published port serving HTTP 200) instead.
  async openTerminal(): Promise<void> {
    await this.#clickButton(this.terminalButton);
  }

  async waitForRunning(timeout = 10000): Promise<void> {
    await (await this.runningIndicator).waitForExist({ timeout });
  }

  async waitForStopped(timeout = 20000): Promise<void> {
    await (await this.runningIndicator).waitForExist({ timeout, reverse: true });
  }

  // Deploys to the named host and resolves once the deployment is live (undeploy button shown).
  async deployTo(host: string, { timeout = 120000 }: { timeout?: number } = {}): Promise<void> {
    const start = Date.now();
    await this.#clickButton(this.deployButton);

    const select = await this.hostSelect;
    await select.waitForDisplayed({ timeout: 5000 });
    await select.selectByVisibleText(host);

    await this.#clickButton(this.root.$("button=Deploy"));

    // Race: undeploy button (success) vs error div (backend failure). Without this, a failed
    // deploy (SSH not reachable, image build error, …) silently burns the full timeout because
    // the undeploy button never appears.
    let errorText = "";
    await browser.waitUntil(
      async () => {
        const err = await this.root.$("div.text-destructive.text-xs");
        if (await err.isExisting()) {
          errorText = await err.getText();
          return true;
        }
        return (await this.undeployButton).isDisplayed();
      },
      {
        timeout,
        interval: 1000,
        timeoutMsg: `deploy to "${host}" did not complete within ${timeout}ms — undeploy button never appeared and no error was shown`,
      }
    );
    if (errorText) throw new Error(`deploy to "${host}" failed: ${errorText}`);

    const remaining = timeout - (Date.now() - start);
    await this.waitForSyncStatus("up-to-date", Math.max(remaining, 60000));
  }

  async undeploy(timeout = 30000): Promise<void> {
    await this.#clickButton(this.undeployButton);
    await (await this.deployButton).waitForDisplayed({ timeout });
  }

  async destroy(): Promise<void> {
    await this.#clickButton(this.destroyButton);
    await this.waitForGone();
  }

  async installPackage(pkg: string, { timeout = 180000 }: { timeout?: number } = {}): Promise<void> {
    const input = await this.packageInput;
    await input.scrollIntoView();
    await input.setValue(pkg);
    await this.#clickButton(this.installButton);
    await this.waitForPackage(pkg, timeout);
  }

  async installedPackages(): Promise<string[]> {
    const chips = await this.root.$$("span.font-mono");
    const names: string[] = [];
    for (const chip of chips) names.push(await chip.getText());
    return names;
  }

  async waitForPackage(pkg: string, timeoutMs = 180000): Promise<void> {
    await browser.waitUntil(async () => (await this.installedPackages()).includes(pkg), {
      timeout: timeoutMs,
      interval: 2000,
      timeoutMsg: `package "${pkg}" did not appear in the row within ${timeoutMs}ms`,
    });
  }

  async toggleSyncOffOn(pauseMs = 500): Promise<void> {
    const sw = await this.syncSwitch;
    await sw.scrollIntoView();
    await sw.click();
    await browser.pause(pauseMs);
    await sw.click();
  }

  async isDeployed(): Promise<boolean> {
    return (await this.undeployButton).isDisplayed();
  }

  async isHostShown(host: string): Promise<boolean> {
    return (await this.deployedHostLabel(host)).isDisplayed();
  }

  get addAgentButton(): Element {
    return this.root.$("button=Add agent");
  }

  agentInstallButton(agentLabel: string): Element {
    return this.root.$(`span*=${agentLabel}`).$("../..").$("button=install");
  }

  agentAuthButton(agentLabel: string): Element {
    return this.root.$(`span=${agentLabel}`).$("..").$("button*=thenticate");
  }

  agentStatusText(agentLabel: string): Element {
    return this.root
      .$(`span=${agentLabel}`)
      .$("..")
      .$("span.text-muted-foreground, span.text-amber-600");
  }

  // The "Start a model to use OpenCode" hint, an <a href="/models"> rendered for a provisioned,
  // auth-less agent when no inference server is running. Distinct from agentStatusText, which only
  // matches <span> status text (the provider-configured state).
  agentProviderHint(agentLabel: string): Element {
    return this.root.$(`span=${agentLabel}`).$("..").$("a[href='/models']");
  }

  async startAgentInstall(agentLabel: string): Promise<void> {
    await this.#clickButton(this.addAgentButton);
    await this.#clickButton(this.agentInstallButton(agentLabel));
  }

  async waitForAgentRow(agentLabel: string, timeoutMs = 600000): Promise<void> {
    await browser.waitUntil(
      async () => (await this.root.$(`span=${agentLabel}`)).isExisting(),
      { timeout: timeoutMs, interval: 3000, timeoutMsg: `agent row for "${agentLabel}" did not appear` }
    );
  }

  async waitForAgentStatus(agentLabel: string, text: string, timeoutMs = 10000): Promise<void> {
    await browser.waitUntil(
      async () => {
        try {
          return (await this.agentStatusText(agentLabel).getText()) === text;
        } catch {
          return false;
        }
      },
      { timeout: timeoutMs, interval: 500, timeoutMsg: `agent status for "${agentLabel}" did not become "${text}"` }
    );
  }

  /**
   * Poll the deployment status text until it equals `targetStatus`. The status span and the
   * host label both carry text-muted-foreground + text-xs, so this checks every match rather
   * than assuming a single one.
   */
  async waitForSyncStatus(targetStatus: string, timeoutMs = 60000): Promise<void> {
    await browser.waitUntil(
      async () => {
        const divs = await this.root.$$("div.text-muted-foreground.text-xs");
        for (const div of divs) {
          if ((await div.getText()) === targetStatus) return true;
        }
        return false;
      },
      {
        timeout: timeoutMs,
        interval: 2000,
        timeoutMsg: `sync status did not reach "${targetStatus}" within ${timeoutMs}ms`,
      }
    );
  }

  // ── Package autocomplete helpers ─────────────────────────────────────────────
  // The suggestion dropdown is mounted to document.body via a Svelte portal action,
  // so it is NOT inside this.root — all selectors below query the document globally.

  // Focus the package input and type `text`, triggering the registry load via onfocus.
  async typeInPackageInput(text: string): Promise<void> {
    const input = await this.packageInput;
    await input.scrollIntoView();
    await input.click();
    await input.setValue(text);
  }

  // Short names of all suggestion items currently visible in the portal dropdown.
  // Items are <button> elements inside a .fixed.bg-popover div mounted at body level.
  async visibleSuggestions(): Promise<string[]> {
    const items = await $$(".fixed.bg-popover button[type='button']");
    const names: string[] = [];
    for (const item of items) {
      const span = await item.$("span.font-mono");
      if (await span.isExisting()) names.push(await span.getText());
    }
    return names;
  }

  // Description text of the suggestion whose short name exactly matches `shortName`.
  // Returns "" when that suggestion is absent or has no description span.
  async suggestionDescription(shortName: string): Promise<string> {
    const items = await $$(".fixed.bg-popover button[type='button']");
    for (const item of items) {
      const nameSpan = await item.$("span.font-mono");
      if (!await nameSpan.isExisting()) continue;
      if ((await nameSpan.getText()) !== shortName) continue;
      const desc = await item.$("span.text-muted-foreground");
      return await desc.isExisting() ? desc.getText() : "";
    }
    return "";
  }

  // Click the suggestion item whose short name exactly matches `shortName`.
  // Uses onmousedown (not onclick) — click() dispatches mousedown first, so it fires
  // before the input's onblur, without needing a setTimeout deferral.
  async selectSuggestion(shortName: string): Promise<void> {
    const items = await $$(".fixed.bg-popover button[type='button']");
    for (const item of items) {
      const span = await item.$("span.font-mono");
      if (await span.isExisting() && (await span.getText()) === shortName) {
        await item.scrollIntoView();
        await item.click();
        return;
      }
    }
    throw new Error(`suggestion "${shortName}" not found in the dropdown`);
  }

  // Current value of the package input field.
  async packageInputValue(): Promise<string> {
    return (await this.packageInput).getValue();
  }
}
