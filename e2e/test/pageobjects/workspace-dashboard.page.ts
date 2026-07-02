import { type Element } from "./app.page.js";
import { switchPane } from "../../lib/workspace.js";

// Page object for the AppKind::Dashboard content pane (docs/features/responsive-panel/02_dev.md
// Phase D / 03_e2e.md): a `workspace-<sandbox>-dashboard` child webview, a separate top-level
// document from `main-global` hosting PackageManager.svelte/AgentManager.svelte moved out of the
// projects-list row. Every getter here is scoped to that document's root (no wrapping `<li>`
// unlike project-row.component.ts, since the whole pane belongs to one sandbox).
//
// Specs must call switchToDashboard(sandbox) before using these getters, and switchToAppWebview()
// afterward — the global `before` hook (e2e/wdio.conf.ts) only pins the session to `main-global`
// once per session, not per test, so a spec that switches away must switch back itself.
class WorkspaceDashboardPage {
  // ── Window/document switching ──────────────────────────────────────────────

  // Poll every window handle until one is this sandbox's dashboard route (mirrors the technique
  // e2e/wdio.conf.ts's global `before` hook uses to find `main-global`, extended to also match the
  // `sandbox` query param since multiple sandboxes' dashboards can coexist).
  async switchToDashboard(sandbox: string, timeoutMs = 15000): Promise<void> {
    await browser.waitUntil(
      async () => {
        // Bring the pane to the front in the Tauri app: `embed_dashboard`/`switch_pane` on the Rust
        // side call WebView2's `hide()` on every pane but the active one, which stops it from
        // painting, so retargeting only the WebDriver window handle (below) leaves it hidden and
        // devtools operations needing layout (scrollIntoView, click) hang until wdio's own timeout.
        // Retried inside this loop (not called once upfront) because callers race
        // `row.openWorkspace()`, which only clicks the button and returns before the backend's
        // async `open_workspace` command has necessarily registered the dashboard pane yet — an
        // early call throws WorkspaceError::NotOpen rather than a pane that just isn't shown.
        try {
          await switchPane(sandbox, "dashboard");
        } catch {
          return false;
        }
        for (const handle of await browser.getWindowHandles()) {
          await browser.switchToWindow(handle);
          const { pathname, search } = await browser.execute(() => ({
            pathname: window.location.pathname,
            search: window.location.search,
          }));
          if (pathname === "/workspace/dashboard" && new URLSearchParams(search).get("sandbox") === sandbox) {
            return true;
          }
        }
        return false;
      },
      {
        timeout: timeoutMs,
        interval: 500,
        timeoutMsg: `dashboard webview for "${sandbox}" never appeared among the window handles`,
      },
    );
  }

  // Switch back to the app webview (main-global) — the same handle the global `before` hook picks.
  async switchToAppWebview(): Promise<void> {
    for (const handle of await browser.getWindowHandles()) {
      await browser.switchToWindow(handle);
      const pagePath = await browser.execute(() => window.location.pathname);
      if (!pagePath.startsWith("/workspace")) return;
    }
    throw new Error("[e2e] no app webview found among the window handles");
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

  async installPackage(pkg: string, { timeout = 180000 }: { timeout?: number } = {}): Promise<void> {
    const input = await this.packageInput;
    await input.scrollIntoView();
    await input.setValue(pkg);
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
    const input = await this.packageInput;
    await input.scrollIntoView();
    await input.click();
    await input.setValue(text);
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
}

export default new WorkspaceDashboardPage();
