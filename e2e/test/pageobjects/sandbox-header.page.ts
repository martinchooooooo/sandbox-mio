import { type Element } from "./app.page.js";
import sidebar from "./sidebar.page.js";
import { PANEL_RENDER, SUBPROCESS } from "../../lib/timeouts.js";

export type HeaderApp = "dashboard" | "terminal" | "agentweb" | "agentchat";

/**
 * The single sandbox header (src/lib/workspace/SandboxHeader.svelte), a DOM region of
 * `main-navigation-panel` above the Manage dashboard or a pane. It exists only for the sandbox in
 * view, so every action first brings `sandbox` into view through the sidebar when another sandbox
 * or a page is showing; that restores the sandbox's last view before the action runs.
 */
class SandboxHeaderPage {
  /** The header of whichever sandbox is in view; absent while a routed page shows. */
  get current(): Element {
    return $("div[data-sandbox-header]");
  }

  root(sandbox: string): Element {
    return $(`div[data-sandbox-header][data-sandbox='${sandbox}']`);
  }

  /** The sandbox in view, or null while a routed page shows. */
  async sandboxInView(): Promise<string | null> {
    const header = await this.current;
    if (!(await header.isExisting())) return null;
    return header.getAttribute("data-sandbox");
  }

  tab(sandbox: string, kind: HeaderApp): Element {
    return this.root(sandbox).$(`button[data-app='${kind}']`);
  }

  closeSandboxButton(sandbox: string): Element {
    return this.root(sandbox).$("button[data-close-sandbox]");
  }

  startButton(sandbox: string): Element {
    return this.root(sandbox).$("button=start");
  }

  stopButton(sandbox: string): Element {
    return this.root(sandbox).$("button=stop");
  }

  /** The header's status dot while the container runs (`statusDotClass` in src/lib/sandbox.ts). */
  runningIndicator(sandbox: string): Element {
    return this.root(sandbox).$("span.bg-green-500");
  }

  /** Starts `sandbox` from its header, leaving whatever pane is shown in place (unlike a Home card). */
  async start(sandbox: string): Promise<void> {
    await this.#clickAction(sandbox, this.startButton(sandbox));
  }

  async stop(sandbox: string): Promise<void> {
    await this.#clickAction(sandbox, this.stopButton(sandbox));
  }

  async waitForRunning(sandbox: string, timeout = SUBPROCESS): Promise<void> {
    await (await this.runningIndicator(sandbox)).waitForExist({ timeout });
  }

  async waitForStopped(sandbox: string, timeout = SUBPROCESS): Promise<void> {
    await (await this.runningIndicator(sandbox)).waitForExist({ timeout, reverse: true });
  }

  async #clickAction(sandbox: string, button: Element): Promise<void> {
    await this.ensureInView(sandbox);
    const el = await button;
    await el.waitForClickable({ timeout: PANEL_RENDER });
    await el.click();
  }

  /** StatusRow's error line for the header's own actions (Start, Stop, Folder). */
  errorRow(sandbox: string): Element {
    return this.root(sandbox).$("div[role='status'].text-destructive");
  }

  /** Which header tab is current: `aria-current="page"` marks Manage or the active pane's tab. */
  async isTabActive(sandbox: string, kind: HeaderApp): Promise<boolean> {
    const tab = await this.tab(sandbox, kind);
    return (await tab.isExisting()) && (await tab.getAttribute("aria-current")) === "page";
  }

  async ensureInView(sandbox: string): Promise<void> {
    if (await (await this.root(sandbox)).isDisplayed()) return;
    await sidebar.selectSandbox(sandbox, PANEL_RENDER);
  }

  async clickTab(sandbox: string, kind: HeaderApp): Promise<void> {
    await this.ensureInView(sandbox);
    const tab = await this.tab(sandbox, kind);
    await tab.waitForExist({ timeout: PANEL_RENDER });
    await tab.scrollIntoView();
    await tab.click();
  }

  /** Closes every pane of `sandbox`; the app lands on Home unless another sandbox's pane takes over. */
  async closeSandbox(sandbox: string): Promise<void> {
    await this.ensureInView(sandbox);
    const button = await this.closeSandboxButton(sandbox);
    await button.waitForClickable({ timeout: PANEL_RENDER });
    await button.click();
  }
}

export default new SandboxHeaderPage();
