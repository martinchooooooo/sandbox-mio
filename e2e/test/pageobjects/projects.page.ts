import AppPage, { type Element } from "./app.page.js";
import ProjectRow from "./project-row.component.js";
import { COMMAND, PODMAN_RECOVERY, UI_SETTLE } from "../../lib/timeouts.js";

class ProjectsPage extends AppPage {
  get navLink(): Element {
    return $("[role='tab'][data-value='/projects']");
  }

  // docs/features/sandbox-list-create
  get createRow(): Element {
    return $("li[data-create-row]");
  }

  get nameInput(): Element {
    return this.createRow.$("input[placeholder='my-project']");
  }

  get createButton(): Element {
    return this.createRow.$("button=Create");
  }

  get useExistingFolderLink(): Element {
    return this.createRow.$("button*=existing folder");
  }

  // Never click this: it spawns a real, blocking OS dialog. Specs assert its presence or absence to
  // confirm the mode toggle (docs/features/sandbox-list-create/03_e2e.md).
  get chooseFolderButton(): Element {
    return this.createRow.$("button*=Choose folder");
  }

  get backToNameButton(): Element {
    return this.createRow.$("[aria-label='Back']");
  }

  async useExistingFolderMode(): Promise<void> {
    await (await this.useExistingFolderLink).click();
  }

  // docs/features/microsandbox-provider — the create row's provider picker. Keyed on the attribute
  // rather than the label, which is still a hard-coded English literal pending the i18n pass.
  providerOption(provider: string): Element {
    return this.createRow.$(`button[data-provider-option='${provider}']`);
  }

  /** Select which provider the next `createProject()` creates on. */
  async chooseProvider(provider: string): Promise<void> {
    const btn = await this.providerOption(provider);
    await btn.waitForClickable({ timeout: UI_SETTLE });
    await btn.click();
  }

  // ── Podman startup-recovery gate ─────────────────────────────────────────────
  // This button's presence is the gate signal, not the Create button: the create form stays
  // mounted while the recovery screen replaces the sandbox list.
  get startPodmanButton(): Element {
    return $("button=Start Podman");
  }

  get podmanNotRunningText(): Element {
    return $("p=Podman machine is not running");
  }

  get retryButton(): Element {
    return $("button=Retry");
  }

  async open(timeout = COMMAND): Promise<void> {
    const url = await browser.getUrl();
    if (url.includes("/projects")) return;
    const link = await this.navLink;
    await link.waitForDisplayed({ timeout });
    await link.click();
  }

  /**
   * Call between open() and createProject(). Drives the app's own recovery rather than
   * `podman machine start`, which can leave the rootless socket down. No-op when already ready.
   */
  async ensurePodmanReady(probeWindow = COMMAND, recoverTimeout = PODMAN_RECOVERY): Promise<void> {
    const btn = await this.startPodmanButton;
    try {
      await btn.waitForDisplayed({ timeout: probeWindow });
    } catch {
      return; // gate never appeared within the probe window → Podman is ready
    }
    await btn.click();
    await btn.waitForDisplayed({
      reverse: true,
      timeout: recoverTimeout,
      timeoutMsg:
        "Podman recovery did not clear the 'Start Podman' gate; podman-machine-default failed to " +
        "start or its rootless socket never came up (see start_podman_machine / activate_podman_socket " +
        "in src-tauri, and the activity center for a failed 'podman-startup' task).",
    });
    await this.createButton.waitForDisplayed({ timeout: UI_SETTLE });
  }

  // Widen `timeout` for the first sandbox on a connection: it triggers the one-time base-image
  // build, so the row can take minutes to appear.
  async createProject(name: string, { timeout = COMMAND }: { timeout?: number } = {}): Promise<ProjectRow> {
    await (await this.nameInput).setValue(name);
    const btn = await this.createButton;
    await btn.waitForClickable({ timeout: UI_SETTLE });
    await btn.click();

    const row = this.row(name);
    await row.waitForDisplayed(timeout);
    return row;
  }

  /** Does not assert the row exists yet. */
  row(name: string): ProjectRow {
    return new ProjectRow(name);
  }
}

export default new ProjectsPage();
