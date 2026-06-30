import AppPage, { type Element } from "./app.page.js";
import ProjectRow from "./project-row.component.js";

// The projects screen: reached via the nav link, creates projects and hands out a ProjectRow
// (see project-row.component.ts) for each one in the list.
class ProjectsPage extends AppPage {
  get navLink(): Element {
    return $("a[href='/projects']");
  }

  get nameInput(): Element {
    return $("input[placeholder='my-project']");
  }

  get createButton(): Element {
    return $("button=Create");
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

  // Navigates to the projects screen. No-ops when the browser is already on /projects
  // (the nav link is only rendered on the home page, so waitForDisplayed would time out).
  async open(timeout = 10000): Promise<void> {
    const url = await browser.getUrl();
    if (url.includes("/projects")) return;
    const link = await this.navLink;
    await link.waitForDisplayed({ timeout });
    await link.click();
  }

  // Creates the project and returns its row once the row is displayed. `timeout` covers the wait
  // for the row: the first sandbox on a connection triggers the one-time base-image build, so the
  // row can take minutes to appear and callers must widen the wait accordingly.
  async createProject(name: string, { timeout = 10000 }: { timeout?: number } = {}): Promise<ProjectRow> {
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
