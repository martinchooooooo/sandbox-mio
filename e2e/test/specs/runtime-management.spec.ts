import assert from "node:assert";
import fs from "fs";
import path from "path";

import ProjectsPage from "../pageobjects/projects.page.js";
import ProjectRow from "../pageobjects/project-row.component.js";
import { podman, podmanOk, LOCAL_CONNECTION, REMOTE_CONNECTION } from "../../lib/podman-connection.js";

const WORKSPACE_ROOT = process.env.SANDBOX_WORKSPACE_ROOT;

// The test package: `hello` is tiny, has a stable "Hello, world!" output, and is the package both
// flox/Nix spikes used, so its reproduction is already hand-proven.
const PKG = "hello";
const PKG_OUTPUT = "Hello, world!";

// node:assert (not expect().toBe) so the second arg carries the diagnostic for plain-value checks,
// matching deploy-remote.spec.ts.

// `function ()` (not an arrow) so `this.timeout` raises the suite ceiling above the global 180s:
// the first remote `flox activate` substitutes the closure from the internet on a cold /nix/store.
describe("Sandbox runtime management (flox)", function () {
  this.timeout(600000);

  const projectName = `e2e-runtime-${Date.now()}`;
  const row: ProjectRow = ProjectsPage.row(projectName);
  let workspaceDir: string;
  // flox writes the manifest under <workspace>/.flox/env/manifest.toml.
  let manifestPath: string;

  before(() => {
    expect(WORKSPACE_ROOT).toBeTruthy();
    workspaceDir = path.join(WORKSPACE_ROOT!, projectName);
    manifestPath = path.join(workspaceDir, ".flox", "env", "manifest.toml");
  });

  after(() => {
    // Best-effort cleanup so a failed run leaves no containers behind.
    podman(LOCAL_CONNECTION, ["rm", "-f", projectName], { check: false });
    podman(REMOTE_CONNECTION, ["rm", "-f", projectName], { check: false });
  });

  // Capture the webview state on any failure: the "row vanished" symptom points at an app crash or
  // navigation, so dump the URL + page source + a screenshot to e2e/.diag for inspection.
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

  it("creates a project whose workspace gets a flox environment", async () => {
    await ProjectsPage.open();
    await ProjectsPage.createProject(projectName);

    await browser.waitUntil(() => fs.existsSync(manifestPath), {
      timeout: 60000,
      interval: 1000,
      timeoutMsg: `flox manifest ${manifestPath} did not appear after create`,
    });
    assert(
      fs.existsSync(manifestPath),
      `flox manifest ${manifestPath} missing after create. This can happen when create_sandbox ran ` +
        "but the runtime init (flox init in a throwaway container) failed or the base image lacks flox."
    );
  });

  it("installs a package through the UI and records it in the manifest", async () => {
    await row.installPackage(PKG);

    const manifest = fs.readFileSync(manifestPath, "utf8");
    assert(
      manifest.includes(PKG),
      `manifest.toml does not mention "${PKG}" after install. Contents:\n${manifest}\n` +
        "This can happen when flox install edited a different env, or the install errored but the UI swallowed it."
    );

    assert(
      (await row.installedPackages()).includes(PKG),
      `installed-package list in the row does not show "${PKG}". ` +
        "This can happen when list_packages did not re-read the manifest after install."
    );
  });

  it("runs the installed package inside the local container", async () => {
    const out = podman(LOCAL_CONNECTION, [
      "exec",
      "--workdir",
      "/root/work",
      projectName,
      "flox",
      "activate",
      "--",
      PKG,
    ]);
    assert(
      out.includes(PKG_OUTPUT),
      `"${PKG}" printed "${out}" instead of containing "${PKG_OUTPUT}" in the local container. ` +
        "This can happen when flox edited the manifest but the closure was not realized into the container's /nix/store."
    );
  });

  it("deploys to the remote and the remote container starts", async () => {
    // Generous: the deploy reproduces software from the manifest, whose first remote activation
    // fetches the closure from the binary cache over the network.
    await row.deployTo(REMOTE_CONNECTION, { timeout: 300000 });

    const state = podman(REMOTE_CONNECTION, [
      "inspect",
      projectName,
      "--format",
      "{{.State.Status}}",
    ]);
    assert(
      state === "running",
      `Remote container "${projectName}" is in state "${state}" after deploy, expected "running". ` +
        "This can happen when the flox base image is missing on the remote, so podman create failed."
    );
  });

  it("reproduces the package on the remote from the manifest, not a committed image", async () => {
    const out = podman(REMOTE_CONNECTION, [
      "exec",
      "--workdir",
      "/root/work",
      projectName,
      "flox",
      "activate",
      "--",
      PKG,
    ]);
    assert(
      out.includes(PKG_OUTPUT),
      `"${PKG}" printed "${out}" instead of containing "${PKG_OUTPUT}" on the remote. ` +
        "This can happen when reproduce_remote did not seed the manifest or flox activate could not substitute the closure."
    );

    // The old deploy committed sandbox-mio/<name>:deployed and pushed it. Its absence proves the
    // package came from manifest.lock, not from a transferred image.
    assert(
      !podmanOk(REMOTE_CONNECTION, ["image", "exists", `sandbox-mio/${projectName}:deployed`]),
      `A committed image "sandbox-mio/${projectName}:deployed" exists on the remote. ` +
        "Deploy must reproduce software from the flox manifest, never by shipping a committed image."
    );
  });

  it("carries the manifest into the remote workspace", async () => {
    await row.waitForSyncStatus("up-to-date");

    const remoteManifest = podman(REMOTE_CONNECTION, [
      "exec",
      projectName,
      "cat",
      "/root/work/.flox/env/manifest.toml",
    ]);
    assert(
      remoteManifest.includes(PKG),
      `Remote manifest.toml does not mention "${PKG}". Contents:\n${remoteManifest}\n` +
        "This can happen when the workspace sync had not seeded /root/work/.flox onto the remote yet."
    );
  });

  it("destroy removes the container but leaves the workspace and manifest intact", async () => {
    await row.destroy();

    assert(
      !podmanOk(LOCAL_CONNECTION, ["inspect", projectName]),
      `Local container "${projectName}" still exists after destroy.`
    );
    assert(
      fs.existsSync(manifestPath),
      `Manifest ${manifestPath} was deleted by destroy. The workspace (and its .flox manifest) must survive destroy.`
    );
  });
});
