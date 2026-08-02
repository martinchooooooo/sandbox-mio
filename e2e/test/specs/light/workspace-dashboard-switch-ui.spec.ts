import assert from "node:assert";

import ProjectsPage from "../../pageobjects/projects.page.js";
import workspaceDashboard from "../../pageobjects/workspace-dashboard.page.js";
import { captureDiagOnFailure } from "../../../lib/diag.js";

// LIGHT spec (mock backend) for src/lib/workspace/packages.svelte.ts. Reproduces the fixed regression:
// switching directly between two Dashboards (no Terminal/AgentWeb pane ever opened) skipped the
// `dashboardSandbox=null` reset, so the reused (non-remounted) WorkspaceDashboard kept the previous sandbox's state.
describe("Dashboard package list follows the active sandbox on direct switch (mock backend)", function () {
  this.timeout(60000);

  const nameA = `e2e-dash-switch-a-${Date.now()}`;
  const nameB = `e2e-dash-switch-b-${Date.now()}`;
  const rowA = ProjectsPage.row(nameA);
  const rowB = ProjectsPage.row(nameB);
  const PKG = "dash-switch-marker";

  captureDiagOnFailure();

  it("installs a package into sandbox A's Dashboard", async () => {
    await ProjectsPage.open();
    await ProjectsPage.createProject(nameA);
    await rowA.start();
    await rowA.waitForRunning();
    await rowA.openWorkspace();
    await workspaceDashboard.waitForRendered(nameA);
    await workspaceDashboard.installPackage(PKG, { timeout: 5000 });
  });

  it("switching directly to sandbox B's Dashboard does not show A's package", async () => {
    await ProjectsPage.createProject(nameB);
    await rowB.start();
    await rowB.waitForRunning();
    // Neither sandbox has opened a Terminal/AgentWeb pane, so `active_app` stays null throughout
    // this click — the exact transition that never remounted WorkspaceDashboard pre-fix.
    await rowB.openWorkspace();
    await workspaceDashboard.waitForRendered(nameB);

    const packages = await workspaceDashboard.installedPackages();
    assert(
      !packages.includes(PKG),
      `Sandbox B's Dashboard shows "${PKG}", which was only installed in A. Got: [${packages.join(", ")}]. ` +
        "This means the packages store leaked across sandboxes, or the Dashboard reused A's stale " +
        "state instead of fetching B's.",
    );
  });

  it("switching back to sandbox A's Dashboard still shows its installed package", async () => {
    await rowA.openWorkspace();
    await workspaceDashboard.waitForRendered(nameA);
    await workspaceDashboard.waitForPackage(PKG, 5000);
  });
});
