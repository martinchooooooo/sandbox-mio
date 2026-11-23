import { type Element } from "./app.page.js";
import sidebar from "./sidebar.page.js";
import workspaceDashboard from "./workspace-dashboard.page.js";
import { PANEL_RENDER, SUBPROCESS } from "../../lib/timeouts.js";

export type SandboxApp = "dashboard" | "terminal" | "agentweb" | "agentchat";

/**
 * The sandbox in view: the content root `div[data-sandbox-view]` (SandboxView.svelte), its apps
 * group in the left sidebar (`[data-sandbox-apps]`, AppSidebar.svelte) and, on the Dashboard only,
 * the title bar (`[data-sandbox-title-bar]`, SandboxTitleBar.svelte) holding Start/Stop beside
 * the status text and the "..." menu (Restart, Close, Delete sandbox). The sidebar is the only
 * chrome; there is no header bar. Only the sandbox in view has an apps group, so every action first
 * brings `sandbox` into view through the sidebar (restoring its last view) and, for Start/Stop,
 * then its Dashboard.
 */
class SandboxViewPage {
  /** The view root of whichever sandbox is in view; absent while a routed page shows. */
  get current(): Element {
    return $("div[data-sandbox-view]");
  }

  root(sandbox: string): Element {
    return $(`div[data-sandbox-view='${sandbox}']`);
  }

  /** The sandbox in view, or null while a routed page shows. */
  async sandboxInView(): Promise<string | null> {
    const view = await this.current;
    if (!(await view.isExisting())) return null;
    return view.getAttribute("data-sandbox-view");
  }

  /** The sidebar's apps group; exists only for the sandbox in view. */
  appsGroup(sandbox: string): Element {
    return $(`[data-sandbox-apps='${sandbox}']`);
  }

  /** The sidebar app child: Dashboard, or an app that has an open pane (apps launch from the Overview cards). */
  tab(sandbox: string, kind: SandboxApp): Element {
    return this.appsGroup(sandbox).$(`button[data-app='${kind}']`);
  }

  /** The close button of an open pane's sidebar child (expanded sidebar, on hover/focus/active). */
  closeTabButton(sandbox: string, kind: Exclude<SandboxApp, "dashboard">): Element {
    return this.appsGroup(sandbox).$(`button[data-close-tab='${kind}']`);
  }

  /** The sidebar row's close button for the sandbox in view (expanded sidebar only). */
  closeSandboxButton(sandbox: string): Element {
    return $(`button[data-nav-sandbox='${sandbox}']`).parentElement().$("button[data-close-sandbox]");
  }

  get titleBar(): Element {
    return $("[data-sandbox-title-bar]");
  }

  get startButton(): Element {
    return this.titleBar.$("button[data-sandbox-start]");
  }

  get menuTrigger(): Element {
    return this.titleBar.$("button[data-sandbox-menu]");
  }

  get stopButton(): Element {
    return this.titleBar.$("button[data-sandbox-stop]");
  }

  // The menu and the dialog are portalled to the document root, so they are queried globally once open.
  get destroyItem(): Element {
    return $("[role='menu'] [data-sandbox-destroy]");
  }

  get destroyDialog(): Element {
    return $("[data-destroy-dialog]");
  }

  get destroyConfirm(): Element {
    return $("[data-destroy-dialog] button[data-destroy-confirm]");
  }

  get destroyCancel(): Element {
    return $("[data-destroy-dialog] button[data-destroy-cancel]");
  }

  get restartItem(): Element {
    return $("[data-sandbox-restart]");
  }

  get closeMenuItem(): Element {
    return $("[role='menu'] [data-close-sandbox]");
  }

  /** The sandbox's status dot while the container runs (`statusDotClass` in src/lib/sandbox.ts), in its sidebar row. */
  runningIndicator(sandbox: string): Element {
    return $(`button[data-nav-sandbox='${sandbox}']`).$("span.bg-green-500");
  }

  /** Starts `sandbox` from the Dashboard title bar. */
  async start(sandbox: string): Promise<void> {
    await this.ensureDashboard(sandbox);
    const el = await this.startButton;
    await el.waitForClickable({ timeout: PANEL_RENDER });
    await el.click();
  }

  /** Stops `sandbox` through the Dashboard title bar's Stop button. */
  async stop(sandbox: string): Promise<void> {
    await this.ensureDashboard(sandbox);
    const el = await this.stopButton;
    await el.waitForClickable({ timeout: PANEL_RENDER });
    await el.click();
  }

  /** Opens the delete confirmation from the title bar's menu. */
  async openDestroyDialog(sandbox: string): Promise<void> {
    await this.ensureDashboard(sandbox);
    await this.openMenu();
    const item = await this.destroyItem;
    await item.waitForClickable({ timeout: PANEL_RENDER });
    await item.click();
    await (await this.destroyDialog).waitForDisplayed({ timeout: PANEL_RENDER });
  }

  /** Deletes `sandbox` through the menu and the confirmation dialog. */
  async destroy(sandbox: string): Promise<void> {
    await this.openDestroyDialog(sandbox);
    const confirm = await this.destroyConfirm;
    await confirm.waitForClickable({ timeout: PANEL_RENDER });
    await confirm.click();
  }

  async openMenu(): Promise<void> {
    const trigger = await this.menuTrigger;
    await trigger.waitForClickable({ timeout: PANEL_RENDER });
    await trigger.click();
    await (await $("[role='menu']")).waitForDisplayed({ timeout: PANEL_RENDER });
  }

  async waitForRunning(sandbox: string, timeout = SUBPROCESS): Promise<void> {
    await (await this.runningIndicator(sandbox)).waitForExist({ timeout });
  }

  async waitForStopped(sandbox: string, timeout = SUBPROCESS): Promise<void> {
    await (await this.runningIndicator(sandbox)).waitForExist({ timeout, reverse: true });
  }

  /** StatusRow's error line of the title bar (lifecycle failures and Open folder errors); Dashboard only. */
  errorRow(): Element {
    return this.titleBar.$("div[role='status'].text-destructive");
  }

  /** Which app is current: `aria-current="page"` marks Dashboard or the active pane's child. */
  async isTabActive(sandbox: string, kind: SandboxApp): Promise<boolean> {
    const tab = await this.tab(sandbox, kind);
    return (await tab.isExisting()) && (await tab.getAttribute("aria-current")) === "page";
  }

  async ensureInView(sandbox: string): Promise<void> {
    if (await (await this.root(sandbox)).isDisplayed()) return;
    await sidebar.selectSandbox(sandbox, PANEL_RENDER);
  }

  /** Brings `sandbox`'s Dashboard (title bar) into view, clicking the sidebar Dashboard child if a pane shows. */
  async ensureDashboard(sandbox: string): Promise<void> {
    await this.ensureInView(sandbox);
    if (await (await this.titleBar).isDisplayed()) return;
    await this.clickTab(sandbox, "dashboard");
    await (await this.titleBar).waitForDisplayed({ timeout: PANEL_RENDER });
  }

  /** Launches (or switches to) an app from its Overview card; the sidebar lists it once open. */
  async launchFromCard(sandbox: string, kind: Exclude<SandboxApp, "dashboard">): Promise<void> {
    await this.ensureDashboard(sandbox);
    await workspaceDashboard.waitForRendered(sandbox);
    await workspaceDashboard.openSection("overview");
    await workspaceDashboard.launchApp(kind);
  }

  /** Clicks the sidebar child; an app without an open pane has none, so it is launched from its card. */
  async clickTab(sandbox: string, kind: SandboxApp): Promise<void> {
    await this.ensureInView(sandbox);
    if (kind !== "dashboard" && !(await (await this.tab(sandbox, kind)).isExisting())) {
      return this.launchFromCard(sandbox, kind);
    }
    const tab = await this.tab(sandbox, kind);
    await tab.waitForExist({ timeout: PANEL_RENDER });
    await tab.scrollIntoView();
    await tab.click();
  }

  /** Closes `kind`'s pane through its sidebar close button (hover reveals it). */
  async closeTab(sandbox: string, kind: Exclude<SandboxApp, "dashboard">): Promise<void> {
    await this.ensureInView(sandbox);
    await (await this.tab(sandbox, kind)).moveTo();
    const button = await this.closeTabButton(sandbox, kind);
    await button.waitForClickable({ timeout: PANEL_RENDER });
    await button.click();
  }

  /** Closes every pane of `sandbox`; the app lands on Home unless another sandbox's pane takes over. */
  async closeSandbox(sandbox: string): Promise<void> {
    await this.ensureInView(sandbox);
    await (await $(`button[data-nav-sandbox='${sandbox}']`)).moveTo();
    const button = await this.closeSandboxButton(sandbox);
    await button.waitForClickable({ timeout: PANEL_RENDER });
    await button.click();
  }
}

export default new SandboxViewPage();
