import { type Element } from "./app.page.js";

// Page object for a sandbox's Dashboard content (docs/features/consolidate-app-container/02_dev.md
// Phase C): a DOM region of the single `main-navigation-panel` webview (WorkspaceDashboard.svelte),
// not a separate webview any more — every getter here just queries that one document. WebDriver's
// session never needs to switch windows to reach it.
class WorkspaceDashboardPage {
  // Wait for this sandbox's Dashboard content to actually be rendered. Dashboard visibility is
  // derived client-side state (activeAppLabel() === null && activeSandbox === sandbox,
  // src/routes/+layout.svelte), and even once shown, WorkspaceDashboard.svelte's own onMount still
  // needs to resolve `invoke("list_sandboxes")` and populate `workspacePath` before PackageManager/
  // AgentManager/PortsManager mount. Wait for the package input — the first element of that gated
  // block — so callers don't race ahead of it. `sandbox` is accepted (not read from the DOM) purely
  // for the error message; callers are responsible for having navigated to the right sandbox first.
  async waitForRendered(sandbox: string): Promise<void> {
    await this.packageInput.waitForExist({
      timeout: 15000,
      timeoutMsg: `dashboard content for "${sandbox}" never rendered (workspacePath likely never resolved)`,
    });
  }

  // ── App launcher (docs/features/workspace-app-tabs/02_dev.md Phase D) ──────
  // Each launchable app renders a `button[data-app="<kind>"]` above PackageManager/AgentManager.

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

  // The dashboard is its own top-level WebView2 surface (not a DOM subtree of main-global), shown
  // via the Rust side's `switch_pane`/`activate_pane`. A single WebDriver `click()` issued right
  // after that pane becomes the active window can land before WebView2 has actually routed input
  // focus into it — the click "succeeds" with no error, but the element never becomes
  // `document.activeElement`, so the Svelte `onfocus` handler (which fetches suggestions) never
  // fires. Retry the click until focus actually lands instead of trusting the first one.
  async #focusInput(input: Element): Promise<void> {
    const el = await input;
    await el.scrollIntoView();

    await browser.waitUntil(
      async () => {
        await el.click();
        return browser.execute((node) => document.activeElement === node, el);
      },
      {
        timeout: 10000,
        interval: 300,
        timeoutMsg: "input never gained document focus after switching to the dashboard webview",
      },
    );
  }

  async installPackage(pkg: string, { timeout = 180000 }: { timeout?: number } = {}): Promise<void> {
    await this.#focusInput(this.packageInput);
    await (await this.packageInput).setValue(pkg);
    await this.#clickButton(this.installButton);
    await this.waitForPackage(pkg, timeout);
  }

  async installedPackages(): Promise<string[]> {
    const chips = await $$("span.font-mono");
    const names: string[] = [];
    for (const chip of chips) names.push(await chip.getText());
    return names;
  }

  async waitForPackage(pkg: string, timeoutMs = 180000): Promise<void> {
    await browser.waitUntil(async () => (await this.installedPackages()).includes(pkg), {
      timeout: timeoutMs,
      interval: 2000,
      timeoutMsg: `package "${pkg}" did not appear in the dashboard within ${timeoutMs}ms`,
    });
  }

  // ── Package autocomplete ────────────────────────────────────────────────────
  // The suggestion dropdown is mounted to document.body via a Svelte portal action.

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

  async waitForAgentRow(agentLabel: string, timeoutMs = 600000): Promise<void> {
    await browser.waitUntil(async () => (await $(`span=${agentLabel}`)).isExisting(), {
      timeout: timeoutMs,
      interval: 3000,
      timeoutMsg: `agent row for "${agentLabel}" did not appear`,
    });
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
      { timeout: timeoutMs, interval: 500, timeoutMsg: `agent status for "${agentLabel}" did not become "${text}"` },
    );
  }

  // ── Port management (PortsManager.svelte) ──────────────────────────────────
  // Each open forward renders as a row keyed by its `kindLabel` text: the fixed port label
  // (e.g. "Exposed :4000") for a manually-exposed port, or the `.mio/ports.json` label
  // (e.g. "webapp") for a file-declared one. The row's link text is the host URL to dial.

  get exposePortInput(): Element {
    return $("input[placeholder='Expose a port (e.g. 3000)']");
  }

  get exposeButton(): Element {
    return $("button=Expose");
  }

  get noPortsText(): Element {
    return $("span=No ports open.");
  }

  // The row div wrapping a given kindLabel's span (see PortsManager.svelte's `{#each}` block).
  portRow(kindLabel: string): Element {
    return $(`span=${kindLabel}`).$("..");
  }

  async exposePort(port: number): Promise<void> {
    const input = await this.exposePortInput;
    await input.scrollIntoView();
    await input.setValue(String(port));
    await this.#clickButton(this.exposeButton);
  }

  async waitForPortRow(kindLabel: string, timeoutMs = 15000): Promise<void> {
    await browser.waitUntil(async () => (await this.portRow(kindLabel)).isExisting(), {
      timeout: timeoutMs,
      interval: 500,
      timeoutMsg: `port row "${kindLabel}" did not appear`,
    });
  }

  async waitForPortRowGone(kindLabel: string, timeoutMs = 15000): Promise<void> {
    // Check the label span directly, not `portRow()`'s `$("span=…").$("..")` parent chain: once the
    // row is truly gone the base span no longer exists, and chaining `.$("..")` onto a missing
    // element throws ("Can't call $ …") instead of yielding a not-existing element — which would
    // make this negative wait fail on the very success it's waiting for.
    await browser.waitUntil(async () => !(await $(`span=${kindLabel}`).isExisting()), {
      timeout: timeoutMs,
      interval: 500,
      timeoutMsg: `port row "${kindLabel}" is still displayed`,
    });
  }

  // The `http://127.0.0.1:<hostPort>/` link's href carries the host port to dial.
  async portRowHostPort(kindLabel: string): Promise<number> {
    const row = await this.portRow(kindLabel);
    const link = await row.$("a");
    const href = await link.getAttribute("href");
    if (!href) throw new Error(`port row "${kindLabel}" link has no href`);
    const port = new URL(href).port;
    if (!port) throw new Error(`port row "${kindLabel}" link has no port in href "${href}"`);
    return Number(port);
  }

  // Manually-exposed rows show a "Stop exposing" ✕ button; file-declared ones don't.
  async hasCloseButton(kindLabel: string): Promise<boolean> {
    const row = await this.portRow(kindLabel);
    return (await row.$("button[title='Stop exposing']")).isExisting();
  }

  async closePortRow(kindLabel: string): Promise<void> {
    const row = await this.portRow(kindLabel);
    await this.#clickButton(row.$("button[title='Stop exposing']"));
  }
}

export default new WorkspaceDashboardPage();
