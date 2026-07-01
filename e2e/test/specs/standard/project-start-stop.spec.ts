import ProjectsPage from "../../pageobjects/projects.page.js";

describe("Project start and stop", () => {
  it("creates a project, starts it (indicator goes green), then stops it", async () => {
    const projectName = `e2e-${Date.now()}`;

    await ProjectsPage.open();
    const row = await ProjectsPage.createProject(projectName);

    await row.start();
    await row.waitForRunning();
    await expect(row.statusText).toHaveText(/running/);

    await row.stop();
    // Allow up to 20s for the 10s graceful stop + event delivery.
    await row.waitForStopped();
    await expect(row.statusText).not.toHaveText(/running/);
  });
});
