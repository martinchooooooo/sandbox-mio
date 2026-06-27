import assert from "node:assert";
import fs from "fs";
import path from "path";

import ProjectsPage from "../pageobjects/projects.page.js";
import ProjectRow from "../pageobjects/project-row.component.js";
import { podman, podmanOk, LOCAL_CONNECTION, REMOTE_CONNECTION } from "../../lib/podman-connection.js";

const WORKSPACE_ROOT = process.env.SANDBOX_WORKSPACE_ROOT;

// The test tool: `jq` is a tiny static single-binary CLI (no runtime deps, so it runs on the glibc
// base unmodified), resolves through mise's aqua backend, and `jq --version` prints a stable
// "jq-<semver>" line. It is the tool the 2026-06-27 mise spike reproduced byte-identically across
// machines, so its install and cross-machine reproduction are already hand-proven (see the "Spike
// Validation" section of docs/features/sandbox-runtime-management/02_dev.md).
const PKG = "jq";
// `jq --version` prints `jq-1.8.2`; the "jq-" prefix is stable across the patch version.
const PKG_OUTPUT = "jq-";

// node:assert (not expect().toBe) so the second arg carries the diagnostic for plain-value checks,
// matching deploy-remote.spec.ts.

// `function ()` (not an arrow) so `this.timeout` raises the suite ceiling above the global 180s:
// the first install downloads jq from its GitHub release, and the first remote `mise install`
// re-downloads the lock-pinned binary over the network on a cold container.
describe("Sandbox runtime management (mise)", function () {
  this.timeout(600000);

  const projectName = `e2e-runtime-${Date.now()}`;
  const row: ProjectRow = ProjectsPage.row(projectName);
  let workspaceDir: string;
  // mise writes the manifest and lockfile at the workspace root (not a hidden directory).
  let manifestPath: string;
  let lockfilePath: string;

  before(() => {
    expect(WORKSPACE_ROOT).toBeTruthy();
    workspaceDir = path.join(WORKSPACE_ROOT!, projectName);
    manifestPath = path.join(workspaceDir, "mise.toml");
    lockfilePath = path.join(workspaceDir, "mise.lock");
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

  it("creates a project whose workspace gets a mise environment", async () => {
    await ProjectsPage.open();
    await ProjectsPage.createProject(projectName);

    await browser.waitUntil(() => fs.existsSync(manifestPath), {
      timeout: 60000,
      interval: 1000,
      timeoutMsg: `mise manifest ${manifestPath} did not appear after create`,
    });
    assert(
      fs.existsSync(manifestPath),
      `mise manifest ${manifestPath} missing after create. This can happen when create_sandbox ran ` +
        "but the runtime init (host-side write of mise.toml) failed."
    );
    // The empty lockfile must also exist so MISE_LOCKFILE=1 installs can maintain it.
    assert(
      fs.existsSync(lockfilePath),
      `mise lockfile ${lockfilePath} missing after create. Without it, installs cannot record ` +
        "resolved versions, because mise only maintains an already-present lockfile."
    );
  });

  it("installs a package through the UI and records it in the manifest and lockfile", async () => {
    await row.installPackage(PKG);

    const manifest = fs.readFileSync(manifestPath, "utf8");
    assert(
      manifest.includes(PKG),
      `mise.toml does not list "${PKG}" after install. Contents:\n${manifest}\n` +
        "This can happen when `mise use` edited a different config, or the install errored but the UI swallowed it."
    );

    // The lockfile gains the resolved version + checksums, proving the baked MISE_LOCKFILE=1 keeps
    // it in lockstep — this is what makes the later remote reproduction deterministic.
    const lock = fs.readFileSync(lockfilePath, "utf8");
    assert(
      lock.includes(PKG),
      `mise.lock does not mention "${PKG}" after install. Contents:\n${lock}\n` +
        "This can happen when MISE_LOCKFILE=1 was not baked into the base image, or mise.lock was absent at install time."
    );

    assert(
      (await row.installedPackages()).includes(PKG),
      `installed-package list in the row does not show "${PKG}". ` +
        "This can happen when list_packages did not re-read the [tools] table of mise.toml after install."
    );
  });

  it("runs the installed package inside the local container", async () => {
    const out = podman(LOCAL_CONNECTION, [
      "exec",
      "--workdir",
      "/root/work",
      projectName,
      "mise",
      "exec",
      "--",
      PKG,
      "--version",
    ]);
    assert(
      out.includes(PKG_OUTPUT),
      `"${PKG} --version" printed "${out}" instead of containing "${PKG_OUTPUT}" in the local container. ` +
        "This can happen when `mise use` recorded the tool in mise.toml but did not install the binary into MISE_DATA_DIR."
    );
  });

  it("deploys to the remote and the remote container starts", async () => {
    // Generous: the deploy reproduces software from the lockfile, whose first remote install
    // fetches the pinned binary from the upstream release host over the network.
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
        "This can happen when the mise base image is missing on the remote, so podman create failed."
    );
  });

  it("reproduces the package on the remote from the lockfile, not a committed image", async () => {
    const out = podman(REMOTE_CONNECTION, [
      "exec",
      "--workdir",
      "/root/work",
      projectName,
      "mise",
      "exec",
      "--",
      PKG,
      "--version",
    ]);
    assert(
      out.includes(PKG_OUTPUT),
      `"${PKG} --version" printed "${out}" instead of containing "${PKG_OUTPUT}" on the remote. ` +
        "This can happen when reproduce_remote did not seed mise.toml/mise.lock or `mise install` could not fetch the pinned binary."
    );

    // The old deploy committed sandbox-mio/<name>:deployed and pushed it. Its absence proves the
    // package came from mise.lock, not from a transferred image.
    assert(
      !podmanOk(REMOTE_CONNECTION, ["image", "exists", `sandbox-mio/${projectName}:deployed`]),
      `A committed image "sandbox-mio/${projectName}:deployed" exists on the remote. ` +
        "Deploy must reproduce software from mise.toml/mise.lock, never by shipping a committed image."
    );
  });

  it("carries the manifest into the remote workspace", async () => {
    await row.waitForSyncStatus("up-to-date");

    const remoteManifest = podman(REMOTE_CONNECTION, [
      "exec",
      projectName,
      "cat",
      "/root/work/mise.toml",
    ]);
    assert(
      remoteManifest.includes(PKG),
      `Remote mise.toml does not mention "${PKG}". Contents:\n${remoteManifest}\n` +
        "This can happen when the workspace sync had not seeded /root/work/mise.toml onto the remote yet."
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
      `Manifest ${manifestPath} was deleted by destroy. The workspace (and its mise.toml) must survive destroy.`
    );
  });
});
