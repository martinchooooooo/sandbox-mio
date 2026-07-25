import ProjectsPage from "../../pageobjects/projects.page.js";
import { configureMock, useMockControl } from "../../../lib/mock.js";
import { MISE_BASE_IMAGE } from "../../../lib/podman-machine.js";

// LIGHT spec for the scriptable-mock-backend feature's control plane
// (docs/features/scriptable-mock-backend/02_dev.md, Phase C exit criterion). The real MutagenCli
// runs unmodified here; only the `mutagen sync list --template={{json .}}` process call underneath
// it is scripted, so the fixture below travels the genuine parse_session/status_from_session chain
// into SessionInfo, out through sandbox_deployments, and onto the row's badge. A conflicted session
// needs two divergent endpoints on a real deployed sandbox, which the standard tier cannot provoke
// on demand (e2e/AGENTS.md's decision rule 3) — this is the only place the conflict rendering is
// covered.
describe("Sync status — conflict badge from a scripted mutagen sync list (mock backend)", function () {
  this.timeout(30000);

  useMockControl();

  const name = `e2e-sync-conflict-${Date.now()}`;
  const host = "remote-box";
  const row = ProjectsPage.row(name);

  it("renders the conflict status for a session reported as conflicted", async () => {
    // The row itself has to exist for a deployment to attach to: sandbox_deployments is keyed by
    // session name and only renders through a project already in list_sandboxes.
    await configureMock("podman.ps", {
      kind: "return-value",
      stdout: JSON.stringify([
        {
          Id: "conflict-container-id",
          Names: [name],
          State: "running",
          Labels: { "sandbox-mio.workspace": `/tmp/${name}` },
          Image: MISE_BASE_IMAGE,
        },
      ]),
    });

    // Shaped like real `mutagen sync list --template={{json .}}` output, not a typed SessionInfo.
    // A non-empty `conflicts` array is what status_from_session collapses to SyncStatus::Conflict,
    // and `beta.host` is required or parse_session drops the session entirely.
    await configureMock("mutagen.sync-list", {
      kind: "return-value",
      stdout: JSON.stringify([
        {
          name,
          alpha: { host: "" },
          beta: { host },
          status: "watching",
          conflicts: [{ root: "notes.md" }],
        },
      ]),
    });

    await ProjectsPage.open();
    await ProjectsPage.ensurePodmanReady();

    await row.waitForDisplayed();
    // The deployment overlay arrives on the 3s sandbox_deployments poll, not with the row itself.
    await row.waitForSyncStatus("conflict", 15000);
  });
});
