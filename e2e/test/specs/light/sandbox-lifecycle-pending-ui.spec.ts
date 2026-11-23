import ProjectsPage from "../../pageobjects/projects.page.js";
import { configureMock, useMockControl } from "../../../lib/mock.js";

// LIGHT spec for the shared lifecycle store (src/lib/sandbox/lifecycle.svelte.ts): a start clicked on
// a Home card is visible on the sidebar entry too, and clears once the sandbox runs. The mock's
// `delay` behaviour holds `podman start` long enough to observe the in-flight state.
describe("Sandbox lifecycle — pending state is shared across surfaces (mock backend)", function () {
  this.timeout(60000);

  useMockControl();

  const name = `e2e-pending-${Date.now()}`;
  const row = ProjectsPage.row(name);
  const sidebarEntry = () => $(`[data-nav-sandbox="${name}"]`);

  it("shows data-pending=start on the sidebar entry and the card, then clears when running", async () => {
    await ProjectsPage.open();
    await ProjectsPage.ensurePodmanReady();
    await ProjectsPage.createProject(name);
    // Create leaves the sandbox running; stop it so Start is offered.
    await row.stop();

    await configureMock("podman.start", { kind: "delay", ms: 3000, calls: 1 });
    await row.start();

    await browser.waitUntil(async () => (await sidebarEntry().getAttribute("data-pending")) === "start", {
      timeout: 5000,
      timeoutMsg: "sidebar entry never showed the pending start",
    });
    await expect(row.root).toHaveAttribute("data-pending", "start");

    await row.waitForRunning();
    await browser.waitUntil(async () => (await sidebarEntry().getAttribute("data-pending")) === null, {
      timeout: 5000,
      timeoutMsg: "pending state did not clear",
    });
  });

  it("shows data-pending=destroy on the sidebar entry during a destroy, then removes the entry", async () => {
    await configureMock("podman.rm", { kind: "delay", ms: 3000, calls: 1 });
    // row.destroy() resolves only once the card is gone, so it runs while the pending state is polled.
    const destroyed = row.destroy();

    await browser.waitUntil(async () => (await sidebarEntry().getAttribute("data-pending")) === "destroy", {
      timeout: 10000,
      timeoutMsg: "sidebar entry never showed the pending destroy",
    });
    await destroyed;
    await browser.waitUntil(async () => !(await sidebarEntry().isExisting()), {
      timeout: 5000,
      timeoutMsg: "sidebar entry still listed after destroy",
    });
  });
});
