import assert from "node:assert";

import ProjectsPage from "../../pageobjects/projects.page.js";
import sandboxHeader from "../../pageobjects/sandbox-header.page.js";
import workspaceDashboard from "../../pageobjects/workspace-dashboard.page.js";
import { captureDiagOnFailure } from "../../../lib/diag.js";
import { configureMock, useMockControl } from "../../../lib/mock.js";
import { MISE_BASE_IMAGE } from "../../../lib/podman-machine.js";

// Covers the panel's rendering only (docs/features/host-tool-ssh-access/03_e2e.md). A light `create`
// writes a real alias and keypair, so assertions key on this run's name, never on global `~/.ssh`.
describe("SSH access panel renders the handoff (mock backend)", function () {
  this.timeout(90000);

  useMockControl();
  captureDiagOnFailure();

  const name = `e2e-sshpanel-${Date.now()}`;
  const row = ProjectsPage.row(name);

  it("shows the run-unique alias and the running state for a started sandbox", async () => {
    await ProjectsPage.open();
    await ProjectsPage.createProject(name);
    await row.start();
    await row.waitForRunning();
    await row.openWorkspace();
    await workspaceDashboard.waitForRendered(name);
    await workspaceDashboard.openSection("ports");

    assert.strictEqual(await workspaceDashboard.sshAlias(), name);
    await workspaceDashboard.waitForSshState("running");
  });

  it("shows the identity, known_hosts and ProxyCommand values under Details", async () => {
    const identity = await workspaceDashboard.sshDetail("identity");
    assert(identity.endsWith(`/${name}/id_ed25519`), `unexpected identity path: ${identity}`);

    const knownHosts = await workspaceDashboard.sshDetail("known-hosts");
    assert(knownHosts.endsWith(`/${name}/known_hosts`), `unexpected known_hosts path: ${knownHosts}`);

    const proxyCommand = await workspaceDashboard.sshDetail("proxy-command");
    assert(
      proxyCommand.includes(name) && proxyCommand.includes("mio-sshd"),
      `ProxyCommand does not exec into this sandbox: ${proxyCommand}`,
    );
  });

  it("re-fetches on stop and tells the user to start the sandbox", async () => {
    await sandboxHeader.stop(name);
    await sandboxHeader.waitForStopped(name);
    await workspaceDashboard.waitForSshState("stopped");
    assert.strictEqual(await workspaceDashboard.sshAlias(), name);
  });

  it("never reports a deployed sandbox as stopped", async () => {
    // A deployed sandbox's payload always says `running: false`
    // (docs/features/host-tool-ssh-access/04_improvements.md, "Running status for a deployed sandbox").
    const deployed = `e2e-sshpanel-deployed-${Date.now()}`;
    const host = "remote-box";
    await configureMock("podman.ps", {
      kind: "return-value",
      stdout: JSON.stringify([
        {
          Id: "sshpanel-deployed-id",
          Names: [deployed],
          State: "running",
          Labels: { "sandbox-mio.workspace": `/tmp/${deployed}` },
          Image: MISE_BASE_IMAGE,
        },
      ]),
    });
    // Shaped like real `mutagen sync list --template={{json .}}` output (light/sync-status-ui.spec.ts).
    await configureMock("mutagen.sync-list", {
      kind: "return-value",
      stdout: JSON.stringify([{ name: deployed, alpha: { host: "" }, beta: { host }, status: "watching" }]),
    });

    const deployedRow = ProjectsPage.row(deployed);
    await deployedRow.waitForDisplayed();
    // The deployment overlay arrives on the 3s sandbox_deployments poll, not with the row itself.
    await deployedRow.waitForSyncStatus("up-to-date", 15000);
    await deployedRow.openWorkspace();
    await workspaceDashboard.waitForRendered(deployed);
    await workspaceDashboard.openSection("ports");

    await workspaceDashboard.waitForSshState("deployed");
    assert.strictEqual(await workspaceDashboard.sshAlias(), deployed);
    const text = await workspaceDashboard.sshStateText();
    assert(text.includes(host), `the deployed state does not name the deploy host: ${text}`);
  });
});
