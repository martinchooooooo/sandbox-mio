import ProjectsPage from "../../pageobjects/projects.page.js";
import { configureMock, useMockControl } from "../../../lib/mock.js";

// LIGHT spec for scriptable-mock-backend's control plane (docs/features/scriptable-mock-backend/02_dev.md,
// Phase B; e2e/AGENTS.md decision rule 3 — real infra is too slow to provoke this on demand). Only
// `podman start`'s process call is scripted; PodmanCliBackend::start's real check() still produces BackendError::CommandFailed.
describe("Sandbox start failure — error line and retry (mock backend)", function () {
  this.timeout(30000);

  useMockControl();

  const name = `e2e-start-fail-${Date.now()}`;
  const row = ProjectsPage.row(name);

  it("shows the scripted CommandFailed message, then succeeds once the override is consumed", async () => {
    await ProjectsPage.open();
    await ProjectsPage.ensurePodmanReady();
    await ProjectsPage.createProject(name);

    await configureMock("podman.start", {
      kind: "fail-once",
      exit_code: 1,
      stderr: "boom (scripted)",
    });

    await row.start();
    await row.waitForError("boom (scripted)");

    // The fail-once override was consumed by the call above; a retry now hits
    // ScriptedPodmanRunner's unscripted default (empty stdout, exit 0), which `check()` treats as
    // success — the same "podman said nothing happened" default every no-override call gets.
    await row.start();
    await row.waitForRunning();
  });
});
