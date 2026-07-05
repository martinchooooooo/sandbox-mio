import assert from "node:assert";
import fs from "fs";
import path from "path";

import ProjectsPage from "../../pageobjects/projects.page.js";
import workspaceDashboard from "../../pageobjects/workspace-dashboard.page.js";
import { LOCAL_CONNECTION } from "../../../lib/podman-connection.js";
import { captureDiagOnFailure } from "../../../lib/diag.js";
import { useSandboxWorkspace } from "../../../lib/sandbox-suite.js";

// `jq` is a static binary with no runtime deps, resolves quickly via mise's aqua backend,
// and its name prefix matches a small bounded set ("jq", "jq-lsp", "jqp") — easy to assert.
const PKG = "jq";

// `function ()` so `this.timeout` raises the suite ceiling above the global 180 s.
// The first `list_registry` call runs `mise registry --json` over `podman exec`, which starts
// the container and returns ~983 entries — allow 60 s just for that round trip.
describe("Package autocomplete", function () {
  this.timeout(300000);

  const { projectName, row, workspaceDir } = useSandboxWorkspace("e2e-autocomplete");

  captureDiagOnFailure();

  it("creates a project and opens its workspace's dashboard pane", async () => {
    await ProjectsPage.open();
    await ProjectsPage.createProject(projectName);
    await expect(row.root).toBeDisplayed();

    // Package management now requires a workspace to be open (Resolved Decision 3,
    // docs/features/responsive-panel/02_dev.md) — there is no inline fallback in the row anymore.
    await row.openWorkspace();
    await workspaceDashboard.waitForRendered(projectName);
  });

  it("shows suggestions after typing a search prefix", async () => {
    // Focus triggers ensureRegistry → invoke("list_registry") → mise registry --json in the container.
    await workspaceDashboard.typeInPackageInput(PKG);

    // Allow up to 60 s for the registry fetch: mise registry --json starts the container if needed.
    await browser.waitUntil(
      async () => (await workspaceDashboard.visibleSuggestions()).length > 0,
      {
        timeout: 60000,
        interval: 1000,
        timeoutMsg:
          `No autocomplete suggestions appeared after typing "${PKG}" within 60 s. ` +
          `Check that the sandbox container exists, the mise base image is staged on ${LOCAL_CONNECTION}, ` +
          "and that list_registry returned entries (run: podman exec <name> mise registry --json).",
      }
    );

    assert(
      (await workspaceDashboard.visibleSuggestions()).includes(PKG),
      `"${PKG}" was not among the visible suggestions. Got: ${(await workspaceDashboard.visibleSuggestions()).join(", ")}. ` +
        "This can happen when the client-side substring filter dropped the entry or list_registry returned an empty list."
    );
  });

  it("shows a description for the jq suggestion", async () => {
    // A non-empty description proves mise registry --json (not the plain-text format) was used,
    // and that the description field survived the Rust→frontend serialisation round trip.
    const desc = await workspaceDashboard.suggestionDescription(PKG);
    assert(
      desc.length > 0,
      `The "${PKG}" suggestion has no description text. ` +
        "This can happen when list_registry parsed the plain-text registry format instead of --json, " +
        "or when the description field was dropped during Rust→frontend serialisation."
    );
  });

  it("populates the input when a suggestion is selected", async () => {
    // The onmousedown handler on each suggestion button calls e.preventDefault(), which prevents
    // the blur event from stealing focus away from the input before the handler sets packageInput.
    await workspaceDashboard.selectSuggestion(PKG);

    // Poll briefly: Svelte's reactive binding may need one tick to flush the DOM value attribute.
    await browser.waitUntil(
      async () => (await workspaceDashboard.packageInputValue()) === PKG,
      {
        timeout: 2000,
        interval: 100,
        timeoutMsg:
          `packageInput did not show "${PKG}" after selecting the suggestion. ` +
          "This can happen when the onmousedown handler did not fire before the blur closed the dropdown.",
      }
    );

    assert.strictEqual(
      await workspaceDashboard.packageInputValue(),
      PKG,
      `Expected package input to contain "${PKG}" after suggestion selection.`
    );
  });

  it("installs successfully after autocomplete selection", async () => {
    // The input already contains PKG from the previous step; installPackage calls setValue(PKG)
    // (idempotent) then clicks Install and waits for the package chip to appear.
    await workspaceDashboard.installPackage(PKG);

    assert(
      (await workspaceDashboard.installedPackages()).includes(PKG),
      `"${PKG}" chip did not appear in the dashboard pane after install. ` +
        "This can happen when install_package errored or list_packages did not re-read mise.toml."
    );

    const manifestPath = path.join(workspaceDir, "mise.toml");
    const manifest = fs.readFileSync(manifestPath, "utf8");
    assert(
      manifest.includes(PKG),
      `mise.toml does not list "${PKG}" after install via autocomplete. Contents:\n${manifest}`
    );
  });
});
