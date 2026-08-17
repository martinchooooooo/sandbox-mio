import { type Element } from "./app.page.js";
import workspaceDashboard from "./workspace-dashboard.page.js";
import { COMMAND, DEPLOY, DESTROY_GRACE, SUBPROCESS, SYNC_CONVERGE, UI_SETTLE } from "../../lib/timeouts.js";

// Enough to outlast two of the deployment poll's 3 s re-render windows without masking a button that
// is genuinely inert — a click lost to a re-render is rare, so needing all three means a real defect.
const ACTION_CLICK_ATTEMPTS = 3;

/**
 * One project's `<li>` in the projects list, resolved lazily on each access so the component
 * survives an in-place re-render. Keyed on `data-sandbox` rather than text because the create row
 * shows "Creating <name>…" while a sandbox is in flight, which a text selector would match instead.
 *
 * Deploy/Undeploy/Destroy/Sync moved into WorkspaceDashboard.svelte
 * (docs/features/desktop-app-feel/02_dev.md Phase B); this row only keeps Start/Stop and Open.
 * `deployTo`/`undeploy`/`destroy`/`toggleSyncOffOn` open the Dashboard themselves before acting, so
 * every call site that used them before Phase B is unchanged.
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

  get runningIndicator(): Element {
    return this.root.$("span.bg-green-500");
  }

  get statusText(): Element {
    return this.root.$(".text-muted-foreground");
  }

  deployedHostLabel(host: string): Element {
    return this.root.$(`span*=${host}`);
  }

  // The row keeps a "→ host" label (SandboxItem.svelte) even though the Deploy/Undeploy controls
  // that used to sit beside it moved into WorkspaceDashboard.svelte — this is the still-present
  // signal isDeployed() reads instead of the relocated undeploy button.
  get deploymentIndicator(): Element {
    return this.root.$("span*=→");
  }

  async waitForDisplayed(timeout = COMMAND): Promise<void> {
    await (await this.root).waitForDisplayed({ timeout });
  }

  async waitForGone(timeout = SUBPROCESS): Promise<void> {
    await (await this.root).waitForExist({ timeout, reverse: true });
  }

  // Scrolls first: the row's later controls can sit off-screen.
  async #clickButton(button: Element): Promise<void> {
    const el = await button;
    await el.scrollIntoView();
    // The projects list re-renders on the shared sandbox-state poll (src/lib/sandbox/state.svelte.ts),
    // so a click issued without this can land while the row is being swapped and never reach a handler.
    await el.waitForClickable({ timeout: UI_SETTLE });
    await el.click();
  }

  /**
   * Click a button that invokes a backend command, and confirm the command actually started —
   * retrying the click when it did not. A click lost to the poll's re-render leaves no trace: no
   * busy state, no error, the row simply idle, which then burns the caller's whole budget.
   * `errorRow` is the StatusRow error line for whichever action runner owns this button — the row's
   * own for start/stop, WorkspaceDashboard's shared one for the relocated deploy/undeploy actions.
   */
  async #clickAction(button: Element, action: string, errorRow: Element): Promise<void> {
    for (let attempt = 1; attempt <= ACTION_CLICK_ATTEMPTS; attempt++) {
      await this.#clickButton(button);
      if (await this.#actionStarted(button, errorRow)) return;
      console.warn(`[e2e] "${action}" click ${attempt}/${ACTION_CLICK_ATTEMPTS} did not start the action, retrying...`);
    }
    throw new Error(
      `"${action}" never started after ${ACTION_CLICK_ATTEMPTS} clicks — the row stayed idle with no busy state and no error`
    );
  }

  // Whether the last click reached a handler: the row went busy, surfaced an error, or the button
  // itself went away. Absence of all three within the window means the click was swallowed.
  async #actionStarted(button: Element, errorRow: Element): Promise<boolean> {
    try {
      await browser.waitUntil(
        async () => {
          if (await (await errorRow).isExisting()) return true;
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

  // tauri-driver cannot drive the child webview this opens; assert against workspace_state
  // (e2e/lib/workspace.ts) instead.
  async openWorkspace(): Promise<void> {
    await this.#clickButton(this.workspaceButton);
  }

  // Deploy/undeploy/destroy/sync all act on WorkspaceDashboard.svelte now, so each opens it first
  // (e2e/AGENTS.md's "Dashboard specs" section: wait for render before using its getters).
  async #openDashboard(): Promise<void> {
    await this.openWorkspace();
    await workspaceDashboard.waitForRendered(this.projectName);
  }

  async waitForRunning(timeout = SUBPROCESS): Promise<void> {
    await (await this.runningIndicator).waitForExist({ timeout });
  }

  async waitForStopped(timeout = SUBPROCESS): Promise<void> {
    await (await this.runningIndicator).waitForExist({ timeout, reverse: true });
  }

  /** Resolves only once the deployment is live and synced. */
  async deployTo(host: string, { timeout = DEPLOY }: { timeout?: number } = {}): Promise<void> {
    const start = Date.now();
    await this.#openDashboard();
    await this.#clickButton(workspaceDashboard.deployButton);

    const select = await workspaceDashboard.hostSelect;
    await select.waitForDisplayed({ timeout: UI_SETTLE });
    await select.selectByVisibleText(host);

    await this.#clickAction(workspaceDashboard.deployConfirmButton, `deploy to "${host}"`, workspaceDashboard.errorRow);

    await this.#settle(
      async () => (await workspaceDashboard.undeployButton).isDisplayed(),
      `deploy to "${host}"`,
      timeout,
      workspaceDashboard.errorRow
    );

    const remaining = timeout - (Date.now() - start);
    await this.waitForSyncStatus("up-to-date", Math.max(remaining, SYNC_CONVERGE));
  }

  // undeploy_sandbox (src-tauri/src/lib.rs) chains a flush, an SSH remove, Mutagen teardown and a
  // container start — same budget as deployTo.
  async undeploy(timeout = DEPLOY): Promise<void> {
    await this.#openDashboard();
    await this.#clickAction(workspaceDashboard.undeployButton, "undeploy", workspaceDashboard.errorRow);
    await this.#settle(
      async () => (await workspaceDashboard.deployButton).isDisplayed(),
      "undeploy",
      timeout,
      workspaceDashboard.errorRow
    );
  }

  /**
   * Wait for a deploy-direction action to either succeed or surface StatusRow's error line, and
   * throw with that line's text when it does. Without the error branch a backend failure burns the
   * whole budget and reports only that a button never appeared, discarding the reason.
   *
   * Both directions share this so neither can regain a blind wait the other has already lost.
   */
  async #settle(succeeded: () => Promise<boolean>, action: string, timeout: number, errorRow: Element): Promise<void> {
    let errorText = "";
    await browser.waitUntil(
      async () => {
        const err = await errorRow;
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

  async destroy(timeout = DESTROY_GRACE): Promise<void> {
    await this.#openDashboard();
    await this.#clickButton(workspaceDashboard.destroyButton);
    await this.waitForGone(timeout);
  }

  async toggleSyncOffOn(pauseMs = 500): Promise<void> {
    await this.#openDashboard();
    const sw = await workspaceDashboard.syncSwitch;
    await sw.scrollIntoView();
    await sw.click();
    await browser.pause(pauseMs);
    await sw.click();
  }

  async isDeployed(): Promise<boolean> {
    return (await this.deploymentIndicator).isExisting();
  }

  async isHostShown(host: string): Promise<boolean> {
    return (await this.deployedHostLabel(host)).isDisplayed();
  }

  /**
   * StatusRow's error line for this row's own action runner (start/stop/open-workspace — the
   * controls that stayed here after Phase B). Deploy/undeploy/destroy/sync errors surface through
   * `workspaceDashboard.errorRow` instead, since those actions now run through the Dashboard's
   * shared action runner (docs/features/desktop-app-feel/02_dev.md Phase B).
   */
  get errorRow(): Element {
    return this.root.$("div[role='status'].text-destructive");
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

  /** Checks every match: the status span and the host label share the same classes. */
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
