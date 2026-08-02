import AppPage, { type Element } from "./app.page.js";
import ProjectRow from "./project-row.component.js";
import { COMMAND, PODMAN_RECOVERY, UI_SETTLE } from "../../lib/timeouts.js";

// The projects screen: reached via the nav link, creates projects and hands out a ProjectRow
// (see project-row.component.ts) for each one in the list.
class ProjectsPage extends AppPage {
  get navLink(): Element {
    return $("[role='tab'][data-value='/projects']");
  }

  // The always-present create row, last <li> in the sandbox list (docs/features/sandbox-list-create).
  get createRow(): Element {
    return $("li[data-create-row]");
  }

  get nameInput(): Element {
    return this.createRow.$("input[placeholder='my-project']");
  }

  get createButton(): Element {
    return this.createRow.$("button=Create");
  }

  // The discreet link that reveals the folder picker (initial "name" mode → "folder" mode).
  get useExistingFolderLink(): Element {
    return this.createRow.$("button*=existing folder");
  }

  // Visible only in "folder" mode; clicking it in real automation would spawn a real, blocking OS
  // dialog (see docs/features/sandbox-list-create/03_e2e.md's Existing Test Styles) — specs assert
  // its presence/absence to confirm mode-toggle wiring, but never click it.
  get chooseFolderButton(): Element {
    return this.createRow.$("button*=Choose folder");
  }

  // Resets "folder" mode back to "name" mode, discarding any picked path.
  get backToNameButton(): Element {
    return this.createRow.$("[aria-label='Back']");
  }

  async useExistingFolderMode(): Promise<void> {
    await (await this.useExistingFolderLink).click();
  }

  // ── Podman startup-recovery gate (podman-startup-recovery feature) ───────────
  // Rendered in place of the sandbox list when check_podman_ready reports the default machine is
  // down. The create form above the list stays mounted, so the recovery signal is this button's
  // presence/absence — not the Create button.
  get startPodmanButton(): Element {
    return $("button=Start Podman");
  }

  get podmanNotRunningText(): Element {
    return $("p=Podman machine is not running");
  }

  get retryButton(): Element {
    return $("button=Retry");
  }

  // Navigates to the projects screen. No-ops when the browser is already on /projects.
  async open(timeout = COMMAND): Promise<void> {
    const url = await browser.getUrl();
    if (url.includes("/projects")) return;
    const link = await this.navLink;
    await link.waitForDisplayed({ timeout });
    await link.click();
  }

  // Clears the Podman startup-recovery gate if it is showing, so createProject is not blocked.
  // /projects probes check_podman_ready on mount; when podman-machine-default is down (or its
  // rootless socket is inactive) it renders the "Start Podman" screen in place of the sandbox list.
  // This drives the app's own recovery — which activates the rootless socket that a bare
  // `podman machine start` can leave down — and resolves once the page is usable. No-op (after the
  // probe window) when Podman is already ready. Call after open(), before createProject.
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

  // Creates the project and returns its row once the row is displayed. `timeout` covers the wait
  // for the row: the first sandbox on a connection triggers the one-time base-image build, so the
  // row can take minutes to appear and callers must widen the wait accordingly.
  async createProject(name: string, { timeout = COMMAND }: { timeout?: number } = {}): Promise<ProjectRow> {
    await (await this.nameInput).setValue(name);
    await (await this.createButton).click();

    const row = this.row(name);
    await row.waitForDisplayed(timeout);
    return row;
  }

  // Returns a row component for the named project without asserting it exists yet.
  row(name: string): ProjectRow {
    return new ProjectRow(name);
  }
}

export default new ProjectsPage();
