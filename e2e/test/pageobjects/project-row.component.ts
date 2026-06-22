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

  async waitForRunning(timeout = 10000): Promise<void> {
    await (await this.runningIndicator).waitForExist({ timeout });
  }

  async waitForStopped(timeout = 20000): Promise<void> {
    await (await this.runningIndicator).waitForExist({ timeout, reverse: true });
  }

  // Deploys to the named host and resolves once the deployment is live (undeploy button shown).
  async deployTo(host: string, { timeout = 120000 }: { timeout?: number } = {}): Promise<void> {
    await this.#clickButton(this.deployButton);

    const select = await this.hostSelect;
    await select.waitForDisplayed({ timeout: 5000 });
    await select.selectByVisibleText(host);

    await this.#clickButton(this.root.$("button=Deploy"));
    await (await this.undeployButton).waitForDisplayed({ timeout });
  }

  async undeploy(timeout = 30000): Promise<void> {
    await this.#clickButton(this.undeployButton);
    await (await this.deployButton).waitForDisplayed({ timeout });
  }

  async destroy(): Promise<void> {
    await this.#clickButton(this.destroyButton);
    await this.waitForGone();
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

  /**
   * Poll the deployment status text until it equals `targetStatus`. The status span and the
   * host label both carry text-muted-foreground + text-xs, so this checks every match rather
   * than assuming a single one.
   */
  async waitForSyncStatus(targetStatus: string, timeoutMs = 60000): Promise<void> {
    await browser.waitUntil(
      async () => {
        const spans = await this.root.$$("span.text-muted-foreground.text-xs");
        for (const span of spans) {
          if ((await span.getText()) === targetStatus) return true;
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
}
