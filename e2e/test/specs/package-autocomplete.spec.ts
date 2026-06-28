import assert from "node:assert";
import fs from "fs";
import path from "path";

import ProjectsPage from "../pageobjects/projects.page.js";
import ProjectRow from "../pageobjects/project-row.component.js";
import { podman, LOCAL_CONNECTION } from "../../lib/podman-connection.js";

const WORKSPACE_ROOT = process.env.SANDBOX_WORKSPACE_ROOT;

// `jq` is a static binary with no runtime deps, resolves quickly via mise's aqua backend,
// and its name prefix matches a small bounded set ("jq", "jq-lsp", "jqp") — easy to assert.
const PKG = "jq";

// `function ()` so `this.timeout` raises the suite ceiling above the global 180 s.
// The first `list_registry` call runs `mise registry --json` over `podman exec`, which starts
// the container and returns ~983 entries — allow 60 s just for that round trip.
describe("Package autocomplete", function () {
  this.timeout(300000);

  const projectName = `e2e-autocomplete-${Date.now()}`;
  const row: ProjectRow = ProjectsPage.row(projectName);

  after(() => {
    podman(LOCAL_CONNECTION, ["rm", "-f", projectName], { check: false });
  });

  afterEach(async function () {
    if (this.currentTest?.state !== "failed") return;
    const diag = path.join(process.cwd(), ".diag");
    fs.mkdirSync(diag, { recursive: true });
    const stamp = (this.currentTest?.title ?? "test").replace(/[^a-z0-9]+/gi, "-").slice(0, 40);
    try {
      const url = await browser.getUrl();
      const src = await browser.getPageSource();
      fs.writeFileSync(path.join(diag, `${stamp}.html`), `<!-- url: ${url} -->\n${src}`);
      await browser.saveScreenshot(path.join(diag, `${stamp}.png`));
      console.log(`[diag] url=${url} — dumped ${stamp}.html/.png to ${diag}`);
    } catch (e) {
      console.log(`[diag] could not capture page state: ${String(e)}`);
    }
  });

  it("creates a project", async () => {
    await ProjectsPage.open();
    await ProjectsPage.createProject(projectName);
    await expect(row.root).toBeDisplayed();
  });

  it("shows suggestions after typing a search prefix", async () => {
    // Focus triggers ensureRegistry → invoke("list_registry") → mise registry --json in the container.
    await row.typeInPackageInput(PKG);

    // Allow up to 60 s for the registry fetch: mise registry --json starts the container if needed.
    await browser.waitUntil(
      async () => (await row.visibleSuggestions()).length > 0,
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
      (await row.visibleSuggestions()).includes(PKG),
      `"${PKG}" was not among the visible suggestions. Got: ${(await row.visibleSuggestions()).join(", ")}. ` +
        "This can happen when the client-side substring filter dropped the entry or list_registry returned an empty list."
    );
  });

  it("shows a description for the jq suggestion", async () => {
    // A non-empty description proves mise registry --json (not the plain-text format) was used,
    // and that the description field survived the Rust→frontend serialisation round trip.
    const desc = await row.suggestionDescription(PKG);
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
    await row.selectSuggestion(PKG);

    // Poll briefly: Svelte's reactive binding may need one tick to flush the DOM value attribute.
    await browser.waitUntil(
      async () => (await row.packageInputValue()) === PKG,
      {
        timeout: 2000,
        interval: 100,
        timeoutMsg:
          `packageInput did not show "${PKG}" after selecting the suggestion. ` +
          "This can happen when the onmousedown handler did not fire before the blur closed the dropdown.",
      }
    );

    assert.strictEqual(
      await row.packageInputValue(),
      PKG,
      `Expected package input to contain "${PKG}" after suggestion selection.`
    );
  });

  it("installs successfully after autocomplete selection", async () => {
    // The input already contains PKG from the previous step; installPackage calls setValue(PKG)
    // (idempotent) then clicks Install and waits for the package chip to appear.
    await row.installPackage(PKG);

    assert(
      (await row.installedPackages()).includes(PKG),
      `"${PKG}" chip did not appear in the row after install. ` +
        "This can happen when install_package errored or list_packages did not re-read mise.toml."
    );

    if (WORKSPACE_ROOT) {
      const manifestPath = path.join(WORKSPACE_ROOT, projectName, "mise.toml");
      const manifest = fs.readFileSync(manifestPath, "utf8");
      assert(
        manifest.includes(PKG),
        `mise.toml does not list "${PKG}" after install via autocomplete. Contents:\n${manifest}`
      );
    }
  });
});
