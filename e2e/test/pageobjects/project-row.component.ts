import { type Element } from "./app.page.js";
import { COMMAND, DEPLOY, SUBPROCESS, SYNC_CONVERGE, UI_SETTLE } from "../../lib/timeouts.js";

// Enough to outlast two of the deployment poll's 3 s re-render windows without masking a button that
// is genuinely inert — a click lost to a re-render is rare, so needing all three means a real defect.
const ACTION_CLICK_ATTEMPTS = 3;

/**
 * A single project's row in the projects list. Component object: it wraps one `<li>` and
 * exposes every control inside it (start/stop, deploy/undeploy, destroy, the sync switch,
 * the status indicators) plus the waits that go with them, so specs operate a project by
 * name without knowing the row's markup.
 *
 * The row is resolved lazily on each access via the `data-sandbox` hook the SandboxItem `<li>`
 * carries, so the component keeps working after the frontend re-renders the row in place. The
 * attribute (not a text match) is required because the create row shows "Creating <name>…" while
 * a sandbox is in flight, which a `li*=<name>` text selector would match instead of the real row.
 */
export default class ProjectRow {
  readonly projectName: string;

  constructor(projectName: string) {
    this.projectName = projectName;
  }

  get root(): Element {
    return $(`li[data-sandbox="${this.projectName}"]`);
  }

  get startButton(): Element {
    return this.root.$("button=start");
  }

  get stopButton(): Element {
    return this.root.$("button=stop");
  }

  get workspaceButton(): Element {
    return this.root.$("button=workspace");
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

  /**
   * StatusRow's error line, rendered only while an action's last attempt failed
   * (see createActionRunner() in src/lib/actions.svelte.ts).
   */
  get errorRow(): Element {
    return this.root.$("div[role='status'].text-destructive");
  }

  // The label naming the host a project is deployed to.
  deployedHostLabel(host: string): Element {
    return this.root.$(`span*=${host}`);
  }

  async waitForDisplayed(timeout = COMMAND): Promise<void> {
    await (await this.root).waitForDisplayed({ timeout });
  }

  async waitForGone(timeout = SUBPROCESS): Promise<void> {
    await (await this.root).waitForExist({ timeout, reverse: true });
  }

  // Click a button after scrolling it into view — the row's later controls can sit off-screen.
  async #clickButton(button: Element): Promise<void> {
    const el = await button;
    await el.scrollIntoView();
    // The projects list re-renders on a 3 s deployment poll (src/routes/projects/+page.svelte), so a
    // click issued without this can land while the row is being swapped and never reach a handler.
    await el.waitForClickable({ timeout: UI_SETTLE });
    await el.click();
  }

  /**
   * Click a button that invokes a backend command, and confirm the command actually started —
   * retrying the click when it did not. A click lost to the deployment poll's re-render leaves no
   * trace: no busy state, no error, the row simply idle, which then burns the caller's whole budget.
   */
  async #clickAction(button: Element, action: string): Promise<void> {
    for (let attempt = 1; attempt <= ACTION_CLICK_ATTEMPTS; attempt++) {
      await this.#clickButton(button);
      if (await this.#actionStarted(button)) return;
      console.warn(`[e2e] "${action}" click ${attempt}/${ACTION_CLICK_ATTEMPTS} did not start the action, retrying...`);
    }
    throw new Error(
      `"${action}" never started after ${ACTION_CLICK_ATTEMPTS} clicks — the row stayed idle with no busy state and no error`
    );
  }

  // Whether the last click reached a handler: the row went busy, surfaced an error, or the button
  // itself went away. Absence of all three within the window means the click was swallowed.
  async #actionStarted(button: Element): Promise<boolean> {
    try {
      await browser.waitUntil(
        async () => {
          if (await (await this.errorRow).isExisting()) return true;
          const el = await button;
          if (!(await el.isExisting()) || !(await el.isDisplayed())) return true;
          return (await el.getAttribute("aria-busy")) === "true";
        },
        { timeout: UI_SETTLE, interval: 200 }
      );
      return true;
    } catch {
      return false;
    }
  }

  async start(): Promise<void> {
    await this.#clickButton(this.startButton);
  }

  async stop(): Promise<void> {
    await this.#clickButton(this.stopButton);
  }

  // Clicks the "workspace" button, invoking open_workspace. The embedded terminal renders inside
  // the main window's child webviews (docs/features/embedded-windows/02_dev.md), which
  // tauri-driver cannot drive; specs read state back via workspace_state (e2e/lib/workspace.ts).
  async openWorkspace(): Promise<void> {
    await this.#clickButton(this.workspaceButton);
  }

  async waitForRunning(timeout = SUBPROCESS): Promise<void> {
    await (await this.runningIndicator).waitForExist({ timeout });
  }

  async waitForStopped(timeout = SUBPROCESS): Promise<void> {
    await (await this.runningIndicator).waitForExist({ timeout, reverse: true });
  }

  // Deploys to the named host and resolves once the deployment is live (undeploy button shown).
  async deployTo(host: string, { timeout = DEPLOY }: { timeout?: number } = {}): Promise<void> {
    const start = Date.now();
    await this.#clickButton(this.deployButton);

    const select = await this.hostSelect;
    await select.waitForDisplayed({ timeout: UI_SETTLE });
    await select.selectByVisibleText(host);

    await this.#clickAction(this.root.$("button=Deploy"), `deploy to "${host}"`);

    await this.#settle(
      async () => (await this.undeployButton).isDisplayed(),
      `deploy to "${host}"`,
      timeout
    );

    const remaining = timeout - (Date.now() - start);
    await this.waitForSyncStatus("up-to-date", Math.max(remaining, SYNC_CONVERGE));
  }

  // undeploy_sandbox (src-tauri/src/lib.rs) chains a flush, an SSH remove, Mutagen teardown and a
  // container start — same budget as deployTo.
  async undeploy(timeout = DEPLOY): Promise<void> {
    await this.#clickAction(this.undeployButton, "undeploy");
    await this.#settle(async () => (await this.deployButton).isDisplayed(), "undeploy", timeout);
  }

  /**
   * Wait for a deploy-direction action to either succeed or surface StatusRow's error line, and
   * throw with that line's text when it does. Without the error branch a backend failure burns the
   * whole budget and reports only that a button never appeared, discarding the reason.
   *
   * Both directions share this so neither can regain a blind wait the other has already lost.
   */
  async #settle(succeeded: () => Promise<boolean>, action: string, timeout: number): Promise<void> {
    let errorText = "";
    await browser.waitUntil(
      async () => {
        const err = await this.errorRow;
        if (await err.isExisting()) {
          errorText = await err.getText();
          return true;
        }
        return succeeded();
      },
      {
        timeout,
        interval: 1000,
        timeoutMsg: `${action} did not complete within ${timeout}ms — it neither finished nor showed an error`,
      }
    );
    if (errorText) throw new Error(`${action} failed: ${errorText}`);
  }

  // `podman rm --force` must stop a container that may hold a live foreground process
  // (agent-setup.spec.ts), so it can take the full kill grace.
  async destroy(timeout = SUBPROCESS): Promise<void> {
    await this.#clickButton(this.destroyButton);
    await this.waitForGone(timeout);
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
   * Poll until StatusRow's error line shows exactly `expected`. Cannot be a bare assertion:
   * createActionRunner() sets the error asynchronously, after the click handler's own await settles.
   */
  async waitForError(expected: string, timeoutMs = COMMAND): Promise<void> {
    await browser.waitUntil(
      async () => {
        const el = await this.errorRow;
        return (await el.isExisting()) && (await el.getText()) === expected;
      },
      {
        timeout: timeoutMs,
        interval: 500,
        timeoutMsg: `error line did not become ${JSON.stringify(expected)} within ${timeoutMs}ms`,
      }
    );
  }

  /**
   * Poll the deployment status text until it equals `targetStatus`. The status span and the
   * host label both carry text-muted-foreground + text-xs, so this checks every match rather
   * than assuming a single one.
   */
  async waitForSyncStatus(targetStatus: string, timeoutMs = SYNC_CONVERGE): Promise<void> {
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

}
