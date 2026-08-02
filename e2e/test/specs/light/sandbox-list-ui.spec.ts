import ProjectsPage from "../../pageobjects/projects.page.js";
import { configureMock, useMockControl } from "../../../lib/mock.js";
import { MISE_BASE_IMAGE } from "../../../lib/podman-machine.js";

// LIGHT spec for scriptable-mock-backend's control plane (docs/features/scriptable-mock-backend/02_dev.md,
// Phase B). Scripts `podman.ps` directly (bypassing createProject()) to exercise the real
// PodmanCliBackend::list() parsing against a fabricated line, not ScriptedPodmanRunner's create-tracking default.
describe("Sandbox list — real list() parsing from a scripted podman.ps (mock backend)", function () {
  this.timeout(30000);

  useMockControl();

  const name = `e2e-ps-fixture-${Date.now()}`;
  const workspacePath = `/tmp/${name}`;
  const row = ProjectsPage.row(name);

  it("renders a row sourced entirely from the scripted podman ps output", async () => {
    await configureMock("podman.ps", {
      kind: "return-value",
      stdout: JSON.stringify([
        {
          Id: "fixture-container-id",
          Names: [name],
          State: "running",
          Labels: { "sandbox-mio.workspace": workspacePath },
          Image: MISE_BASE_IMAGE,
        },
      ]),
    });

    await ProjectsPage.open();
    await ProjectsPage.ensurePodmanReady();

    await row.waitForDisplayed();
    await row.waitForRunning();
  });
});
