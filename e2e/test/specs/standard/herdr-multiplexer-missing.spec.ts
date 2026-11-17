import assert from "node:assert";
import path from "path";

import ProjectsPage from "../../pageobjects/projects.page.js";
import ProjectRow from "../../pageobjects/project-row.component.js";
import { podman, podmanOk, LOCAL_CONNECTION } from "../../../lib/podman-connection.js";
import { captureDiagOnFailure } from "../../../lib/diag.js";
import { liveHerdrSessions, terminalView, waitForTerminalAttached } from "../../../lib/herdr.js";
import { workspaceState, openTerminalPane, panesFor } from "../../../lib/workspace.js";

const WORKSPACE_ROOT = process.env.SANDBOX_WORKSPACE_ROOT;
const HERDR = "/usr/local/bin/herdr";
const HERDR_ASIDE = "/usr/local/bin/herdr.off";

// A sandbox on a pre-Herdr image is simulated by moving the binary aside, so the start script's
// `command -v herdr` fails and the exec exits 127 exactly as on an old image
// (docs/features/herdr-multiplexer/03_e2e.md).
describe("Herdr multiplexer missing (old image)", function () {
  this.timeout(600000);

  const name = `e2e-herdr-missing-${Date.now()}`;
  const row: ProjectRow = ProjectsPage.row(name);
  let workspaceDir: string;

  before(() => {
    expect(WORKSPACE_ROOT).toBeTruthy();
    workspaceDir = path.join(WORKSPACE_ROOT!, name);
  });

  // Always put the binary back, then remove the container, so a failed assertion leaves nothing broken.
  after(() => {
    podman(LOCAL_CONNECTION, ["exec", name, "sh", "-c", `[ -e ${HERDR_ASIDE} ] && mv ${HERDR_ASIDE} ${HERDR}`], {
      check: false,
    });
    podman(LOCAL_CONNECTION, ["rm", "-f", name], { check: false });
  });

  captureDiagOnFailure();

  it("creates and starts the sandbox, then hides Herdr from it", async () => {
    await ProjectsPage.open();
    await ProjectsPage.createProject(name, { timeout: 600000 });
    await row.start();
    await row.waitForRunning();
    podman(LOCAL_CONNECTION, ["exec", name, "mv", HERDR, HERDR_ASIDE]);
    assert(!podmanOk(LOCAL_CONNECTION, ["exec", name, "sh", "-c", "command -v herdr"]), "herdr is still on PATH");
  });

  it("opening the Terminal tab shows the recreate message with Retry and starts no session", async () => {
    await row.openWorkspace();
    await openTerminalPane(name, workspaceDir);

    const view = terminalView(name);
    await browser.waitUntil(async () => (await view.isExisting()) && (await view.getAttribute("data-status")) === "failed", {
      timeout: 60000,
      interval: 1000,
      timeoutMsg: `the terminal of "${name}" never reached the failed state although herdr is missing`,
    });
    const message = await view.$("[data-terminal-error]").getText();
    assert(
      message.includes("predates the Herdr terminal") && message.includes("recreate it from the project list"),
      `Unexpected MultiplexerMissing message: ${message}`,
    );
    assert(await view.$("[data-terminal-retry]").isDisplayed(), "no Retry control in the failed terminal");

    assert.deepStrictEqual(liveHerdrSessions(name), [], "a Herdr session exists although the binary was missing");
    assert.strictEqual(panesFor(await workspaceState(), name).length, 1, "the failed pane is still registered");
  });

  it("restoring the binary and clicking Retry attaches", async () => {
    podman(LOCAL_CONNECTION, ["exec", name, "mv", HERDR_ASIDE, HERDR]);
    await terminalView(name).$("[data-terminal-retry]").click();
    await waitForTerminalAttached(name);
    assert.deepStrictEqual(liveHerdrSessions(name), [name]);
  });

  it("destroys the sandbox", async () => {
    await row.destroy();
    assert(!podmanOk(LOCAL_CONNECTION, ["inspect", name]), `Local container "${name}" still exists after destroy.`);
  });
});
