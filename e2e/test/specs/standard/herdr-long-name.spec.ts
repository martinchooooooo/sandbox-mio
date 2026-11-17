import assert from "node:assert";
import path from "path";

import ProjectsPage from "../../pageobjects/projects.page.js";
import ProjectRow from "../../pageobjects/project-row.component.js";
import { podman, podmanOk, LOCAL_CONNECTION } from "../../../lib/podman-connection.js";
import { captureDiagOnFailure } from "../../../lib/diag.js";
import { expectedSessionName, liveHerdrSessions, terminalView, waitForTerminalAttached } from "../../../lib/herdr.js";
import { openTerminalPane, closePane } from "../../../lib/workspace.js";

const WORKSPACE_ROOT = process.env.SANDBOX_WORKSPACE_ROOT;
const NAME_LENGTH = 70;

// Herdr's per-session socket path must fit sun_path, so a sandbox name over 61 characters gets a
// hashed session name (docs/features/herdr-multiplexer/02_dev.md, Resolved Decision 4).
describe("Herdr session name for a long sandbox name", function () {
  this.timeout(600000);

  const prefix = `e2e-herdr-long-${Date.now()}-`;
  const name = prefix + "x".repeat(NAME_LENGTH - prefix.length);
  const session = expectedSessionName(name);
  const row: ProjectRow = ProjectsPage.row(name);
  let workspaceDir: string;

  before(() => {
    expect(WORKSPACE_ROOT).toBeTruthy();
    assert.strictEqual(name.length, NAME_LENGTH);
    workspaceDir = path.join(WORKSPACE_ROOT!, name);
  });

  after(() => {
    podman(LOCAL_CONNECTION, ["rm", "-f", name], { check: false });
  });

  captureDiagOnFailure();

  it("creates and starts a sandbox with a long name", async () => {
    await ProjectsPage.open();
    await ProjectsPage.createProject(name, { timeout: 600000 });
    await row.start();
    await row.waitForRunning();
  });

  it("attaches under the hashed session name, with no socket path error", async () => {
    assert.strictEqual(session.length, 61);
    assert.notStrictEqual(session, name);
    await row.openWorkspace();
    await openTerminalPane(name, workspaceDir);
    await waitForTerminalAttached(name, session);
    assert.deepStrictEqual(liveHerdrSessions(name), [session]);
    const text = (await terminalView(name).getText()) ?? "";
    assert(!text.includes("exceeds capacity of sun_path"), `terminal shows a socket path error: ${text}`);
  });

  it("reattaches to the same session after close and reopen", async () => {
    await closePane(name, "terminal");
    await browser.waitUntil(async () => !(await terminalView(name).isExisting()), {
      timeout: 30000,
      timeoutMsg: "the terminal view stayed mounted after close_pane",
    });
    assert.deepStrictEqual(liveHerdrSessions(name), [session], "closing the tab changed the session");
    await openTerminalPane(name, workspaceDir);
    await waitForTerminalAttached(name, session);
    assert.deepStrictEqual(liveHerdrSessions(name), [session], "reopening started another session");
    assert.notStrictEqual(await terminalView(name).getAttribute("data-status"), "failed");
  });

  it("destroys the sandbox", async () => {
    await row.destroy();
    assert(!podmanOk(LOCAL_CONNECTION, ["inspect", name]), `Local container "${name}" still exists after destroy.`);
  });
});
