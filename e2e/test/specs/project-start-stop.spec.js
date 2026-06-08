describe("Project start and stop", () => {
  it("creates a project, starts it (indicator goes green), then stops it", async () => {
    const projectName = `e2e-${Date.now()}`;

    // Navigate to projects page
    const projectsBtn = await $("a[href='/projects']");
    await projectsBtn.click();

    // Create the project
    const nameInput = await $("input[placeholder='my-project']");
    await nameInput.setValue(projectName);
    const createBtn = await $("button=Create");
    await createBtn.click();

    // Wait for the IPC create + list reload to complete
    const row = await $(`li*=${projectName}`);
    await row.waitForDisplayed({ timeout: 10000 });

    // Start the project
    const startBtn = await row.$("button=start");
    await startBtn.click();

    // Wait for the green indicator (status update arrives via Docker event stream)
    const greenDot = await row.$("span.bg-green-500");
    await greenDot.waitForExist({ timeout: 10000 });

    const statusText = await row.$(".text-muted-foreground");
    await expect(statusText).toHaveText(/running/);

    // Stop the project
    const stopBtn = await row.$("button=stop");
    await stopBtn.click();

    // Wait for indicator to leave green (allow up to 20s for 10s graceful stop + event delivery)
    await greenDot.waitForExist({ timeout: 20000, reverse: true });

    await expect(statusText).not.toHaveText(/running/);
  });
});
