import ProjectsPage from "../../pageobjects/projects.page.js";
import { captureDiagOnFailure } from "../../../lib/diag.js";
import { removeSandbox } from "../../../lib/sandbox-suite.js";

describe("Project start and stop", () => {
  captureDiagOnFailure();

  const projectName = `e2e-${Date.now()}`;

  after(() => removeSandbox(projectName));

  it("creates a project, starts it (indicator goes green), then stops it", async () => {
    await ProjectsPage.open();
    const row = await ProjectsPage.createProject(projectName);

    // Create can leave the sandbox running (agent preinstall starts it); begin from stopped so
    // Start is exercised.
    if (await (await row.runningIndicator).isExisting()) await row.stop();
    await row.waitForStopped();

    await row.start();
    await row.waitForRunning();
    await expect(row.statusText).toHaveText(/running/);

    await row.stop();
    // Allow up to 20s for the 10s graceful stop + event delivery.
    await row.waitForStopped();
    await expect(row.statusText).not.toHaveText(/running/);
  });
});
