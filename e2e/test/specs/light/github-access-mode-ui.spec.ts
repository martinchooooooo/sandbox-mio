import assert from "node:assert";

import ProjectsPage from "../../pageobjects/projects.page.js";
import sandboxView from "../../pageobjects/sandbox-view.page.js";
import workspaceDashboard from "../../pageobjects/workspace-dashboard.page.js";
import { captureDiagOnFailure } from "../../../lib/diag.js";
import { invokeCmd, useMockControl } from "../../../lib/mock.js";
import { COMMAND, PANEL_RENDER } from "../../../lib/timeouts.js";

// LIGHT companion to standard/github-placeholder-swap.spec.ts (docs/features/proxy-credential-secrets/03_e2e.md,
// Scenario 9): the Manage card's opt-out/no-credential states and its sidecar wording; the standard spec proves the
// real placeholder swap behind it.

const SENTINEL = "ghp_light_spec_sentinel_value";
const SIDECAR_TEXT = "token kept securely";

describe("GitHub access card — sidecar wording and opt-out (mock backend)", function () {
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

  it("shows the sidecar wording, toggles opt-out, and reports no credential after delete", async () => {
    await ProjectsPage.goHome();
    await ProjectsPage.open();
    await ProjectsPage.ensurePodmanReady();
    await ProjectsPage.createProject(name);

    // The card only applies to a running sandbox; create leaves it running (the proxy CA injection
    // starts it).
    await ProjectsPage.row(name).openWorkspace();
    await workspaceDashboard.waitForRendered(name);
    await sandboxView.waitForRunning(name);

    await invokeCmd("set_github_token", { token: SENTINEL });

    await workspaceDashboard.openSection("overview");
    await card().waitForExist({ timeout: PANEL_RENDER });

    await waitForState("applied");
    assert((await $("[data-github-access-text]").getText()).includes(SIDECAR_TEXT), "sidecar text is missing from the card");

    await $("[data-github-access-switch]").click();
    await waitForState("opted-out");

    await $("[data-github-access-switch]").click();
    await waitForState("applied");

    await invokeCmd("delete_credential_bundle", { serviceId: "github" });
    await browser.waitUntil(async () => (await card().getAttribute("data-state")) === "no-credential", {
      timeout: COMMAND,
      timeoutMsg: "the card did not return to no-credential after the credential was deleted",
    });
  });
});
