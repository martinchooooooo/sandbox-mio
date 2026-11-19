import assert from "node:assert";

import ProjectsPage from "../../pageobjects/projects.page.js";
import sandboxHeader from "../../pageobjects/sandbox-header.page.js";
import workspaceDashboard from "../../pageobjects/workspace-dashboard.page.js";
import { captureDiagOnFailure } from "../../../lib/diag.js";
import { invokeCmd, useMockControl } from "../../../lib/mock.js";
import { COMMAND, PANEL_RENDER } from "../../../lib/timeouts.js";

// LIGHT companion to standard/github-placeholder-swap.spec.ts (docs/features/proxy-credential-secrets/03_e2e.md,
// Scenario 9): the Manage card's plaintext mode and its opt-out/no-credential states. The mock backend writes no
// sidecar meta.json, so the card is always in plaintext mode here; the standard spec proves sidecar mode.

const SENTINEL = "ghp_light_spec_sentinel_value";
const PLAINTEXT_TEXT = "token stored in sandbox";

describe("GitHub access card — plaintext mode and opt-out (mock backend)", function () {
  this.timeout(60000);
  captureDiagOnFailure();
  useMockControl();

  const name = `e2e-gam-${Date.now()}`;
  const card = () => $("[data-github-access]");

  async function waitForState(state: string): Promise<void> {
    await browser.waitUntil(async () => (await card().isExisting()) && (await card().getAttribute("data-state")) === state, {
      timeout: PANEL_RENDER,
      timeoutMsg: `the GitHub card never reached data-state="${state}"`,
    });
  }

  after(async () => {
    await invokeCmd("delete_credential_bundle", { serviceId: "github" }).catch(() => undefined);
    await invokeCmd("destroy_sandbox", { name, force: true }).catch(() => undefined);
  });

  it("shows plaintext mode, toggles opt-out, and reports no credential after delete", async () => {
    await ProjectsPage.goHome();
    await ProjectsPage.open();
    await ProjectsPage.ensurePodmanReady();
    await ProjectsPage.createProject(name);

    // The card only applies to a running sandbox; before that its state stays "pending".
    await ProjectsPage.row(name).start();
    await ProjectsPage.row(name).openWorkspace();
    await workspaceDashboard.waitForRendered(name);
    await sandboxHeader.waitForRunning(name);

    await invokeCmd("set_github_token", { token: SENTINEL });

    await workspaceDashboard.openSection("overview");
    await card().waitForExist({ timeout: PANEL_RENDER });

    await waitForState("applied");
    assert.strictEqual(await card().getAttribute("data-github-mode"), "plaintext");
    assert((await $("[data-github-access-text]").getText()).includes(PLAINTEXT_TEXT), "plaintext text is missing from the card");

    await $("[data-github-access-switch]").click();
    await waitForState("opted-out");
    assert.strictEqual(await card().getAttribute("data-github-mode"), "plaintext");

    await $("[data-github-access-switch]").click();
    await waitForState("applied");

    await invokeCmd("delete_credential_bundle", { serviceId: "github" });
    await browser.waitUntil(async () => (await card().getAttribute("data-state")) === "no-credential", {
      timeout: COMMAND,
      timeoutMsg: "the card did not return to no-credential after the credential was deleted",
    });
  });
});
