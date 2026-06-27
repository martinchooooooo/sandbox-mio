import assert from "node:assert";
import fs from "fs";
import path from "path";

import ProjectsPage from "../pageobjects/projects.page.js";
import ProjectRow from "../pageobjects/project-row.component.js";
import { podman, podmanOk, LOCAL_CONNECTION, REMOTE_CONNECTION } from "../../lib/podman-connection.js";
import { forwardList } from "../../lib/mutagen.js";

const WORKSPACE_ROOT = process.env.SANDBOX_WORKSPACE_ROOT;

const ZELLIJ_WEB_PORT = "8082";

// `function ()` (not arrow) so `this.timeout` overrides the suite ceiling; the first deploy
// triggers an image save/load and Mutagen agent installation which can run long.
describe("Remote terminal — open via Mutagen forward, undeploy cleans up session", function () {
  this.timeout(600000);

  const projectName = `e2e-remote-terminal-${Date.now()}`;
  const row: ProjectRow = ProjectsPage.row(projectName);
  let workspaceDir: string;
  let tokenPath: string;

  before(() => {
    expect(WORKSPACE_ROOT).toBeTruthy();
    workspaceDir = path.join(WORKSPACE_ROOT!, projectName);
    tokenPath = path.join(workspaceDir, ".mio", "zellij.json");
  });

  after(() => {
    podman(LOCAL_CONNECTION, ["rm", "-f", projectName], { check: false });
    podman(REMOTE_CONNECTION, ["rm", "-f", projectName], { check: false });
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

  it("creates a local sandbox", async () => {
    await ProjectsPage.open();
    await ProjectsPage.createProject(projectName, { timeout: 600000 });
  });

  it("deploys to the remote machine and the remote container starts running", async () => {
    await row.deployTo(REMOTE_CONNECTION);

    const state = podman(REMOTE_CONNECTION, ["inspect", projectName, "--format", "{{.State.Status}}"]);
    assert(
      state === "running",
      `Remote container "${projectName}" on ${REMOTE_CONNECTION} is in state "${state}" after deploy, expected "running". ` +
        "This can happen when the image save/load succeeded but podman create or start failed on the target connection."
    );
    assert(
      await row.isHostShown(REMOTE_CONNECTION),
      `Deployed host label "${REMOTE_CONNECTION}" not visible in the project row. ` +
        "This can happen when sandbox_deployments did not return the session, or the frontend did not re-poll after deploy."
    );
  });

  it("remote container has the zellij port published on 127.0.0.1", () => {
    const bindings = podman(REMOTE_CONNECTION, [
      "inspect", projectName, "--format",
      `{{json (index .HostConfig.PortBindings "${ZELLIJ_WEB_PORT}/tcp")}}`,
    ]);
    let hostIps: string[] = [];
    try {
      const parsed = JSON.parse(bindings) as { HostIp?: string }[] | null;
      hostIps = (parsed ?? []).map((b) => b.HostIp ?? "");
    } catch {
      // Leave hostIps empty; the assertion below reports the raw output.
    }
    assert(
      hostIps.includes("127.0.0.1"),
      `Remote container "${projectName}" has no 127.0.0.1 binding for port ${ZELLIJ_WEB_PORT}. Output:\n${bindings}\n` +
        "This can happen when the remote create call omits --publish 127.0.0.1::8082 (Phase A)."
    );
  });

  it("waits for sync to reach up-to-date", async () => {
    await row.waitForSyncStatus("up-to-date");
  });

  it("opens the terminal: Mutagen session Connected and forwarded port serves HTTP 200", async () => {
    await row.openTerminal();

    await browser.waitUntil(() => fs.existsSync(tokenPath), {
      timeout: 60000,
      interval: 1000,
      timeoutMsg: `token file ${tokenPath} did not appear after openTerminal. ` +
        "open_terminal must write the session file after forward_create succeeds.",
    });

    const session = JSON.parse(fs.readFileSync(tokenPath, "utf8"));
    assert(
      typeof session.token === "string" && session.token.length > 0,
      `Persisted ${tokenPath} has no token. Contents:\n${JSON.stringify(session)}`
    );
    assert(
      typeof session.url === "string" && session.url.startsWith("http://127.0.0.1:"),
      `Persisted ${tokenPath} has no valid url. Contents:\n${JSON.stringify(session)}\n` +
        "open_terminal must write the Mutagen-forwarded Windows-host url alongside the token."
    );

    // Hard proof that Mutagen reached the remote host and the forwarding session is live.
    // Mutagen agent installation on a fresh machine can take several seconds, so poll here.
    let fwdOut = "";
    await browser.waitUntil(
      () => {
        fwdOut = forwardList(`terminal-${projectName}`);
        return fwdOut.includes("Connected");
      },
      {
        timeout: 60000,
        interval: 2000,
        timeoutMsg: `mutagen forward list terminal-${projectName} never showed Connected. Last output:\n${fwdOut}\n` +
          "This can happen when the Mutagen agent bundle is missing from target/debug/ " +
          "or AllowTcpForwarding is disabled in the remote sshd_config.",
      }
    );

    // Decisive end-to-end proof: the forwarded Windows-host port serves the Zellij web client.
    await browser.waitUntil(
      async () => {
        try {
          const res = await fetch(session.url);
          return res.status === 200;
        } catch {
          return false;
        }
      },
      {
        timeout: 30000,
        interval: 1000,
        timeoutMsg: `${session.url} never returned HTTP 200. ` +
          "This can happen when pasta did not route the remote container loopback to the remote host " +
          "loopback (--host-lo-to-ns-lo missing or ineffective on the remote Podman).",
      }
    );
  });

  it("re-opening the terminal is idempotent: session stays Connected and port still serves HTTP 200", async () => {
    await row.openTerminal();

    const fwdOut = forwardList(`terminal-${projectName}`);
    assert(
      fwdOut.includes("Connected"),
      `mutagen forward list terminal-${projectName} lost Connected on re-open. Output:\n${fwdOut}\n` +
        "forward_create must terminate the old session and create a fresh one without breaking connectivity."
    );

    const session = JSON.parse(fs.readFileSync(tokenPath, "utf8"));
    const res = await fetch(session.url);
    assert(
      res.status === 200,
      `${session.url} returned ${res.status} on re-open, expected 200.`
    );
  });

  it("undeploy removes the remote container and terminates the forwarding session", async () => {
    await row.undeploy();

    assert(
      !podmanOk(REMOTE_CONNECTION, ["inspect", projectName]),
      `Remote container "${projectName}" still exists on ${REMOTE_CONNECTION} after undeploy. ` +
        "This can happen when remove_remote failed silently or the wrong connection name was used."
    );

    // undeploy_sandbox must call sync::forward_terminate before tearing down the sync session.
    const fwdOut = forwardList(`terminal-${projectName}`);
    assert(
      !fwdOut.includes("Connected"),
      `Mutagen forwarding session terminal-${projectName} still shows Connected after undeploy. Output:\n${fwdOut}\n` +
        "undeploy_sandbox must call sync::forward_terminate before engine.terminate."
    );
  });

  it("destroy removes the local container", async () => {
    await row.destroy();
    assert(
      !podmanOk(LOCAL_CONNECTION, ["inspect", projectName]),
      `Local container "${projectName}" still exists on ${LOCAL_CONNECTION} after destroy.`
    );
  });
});

describe("Remote terminal — destroy deployed sandbox with active forwarding session", function () {
  this.timeout(600000);

  const projectName = `e2e-remote-term-destroy-${Date.now()}`;
  const row: ProjectRow = ProjectsPage.row(projectName);
  let workspaceDir: string;
  let tokenPath: string;

  // Setup runs in `before` so that a failure (deploy timeout, terminal error) marks the `it`
  // as skipped rather than running it against a missing or half-initialised project row.
  before(async () => {
    expect(WORKSPACE_ROOT).toBeTruthy();
    workspaceDir = path.join(WORKSPACE_ROOT!, projectName);
    tokenPath = path.join(workspaceDir, ".mio", "zellij.json");

    await ProjectsPage.open();
    await ProjectsPage.createProject(projectName, { timeout: 600000 });
    await row.deployTo(REMOTE_CONNECTION);
    await row.waitForSyncStatus("up-to-date");
    await row.openTerminal();

    await browser.waitUntil(() => fs.existsSync(tokenPath), {
      timeout: 60000,
      interval: 1000,
      timeoutMsg: `token file ${tokenPath} did not appear after openTerminal`,
    });

    let fwdOut = "";
    await browser.waitUntil(
      () => {
        fwdOut = forwardList(`terminal-${projectName}`);
        return fwdOut.includes("Connected");
      },
      {
        timeout: 60000,
        interval: 2000,
        timeoutMsg: `mutagen forward list terminal-${projectName} never showed Connected before destroy. Last output:\n${fwdOut}`,
      }
    );
  });

  after(() => {
    podman(LOCAL_CONNECTION, ["rm", "-f", projectName], { check: false });
    podman(REMOTE_CONNECTION, ["rm", "-f", projectName], { check: false });
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
    } catch (e) {
      console.log(`[diag] could not capture page state: ${String(e)}`);
    }
  });

  it("destroy removes both containers and terminates the forwarding session", async () => {
    // destroy_sandbox handles deployed sandboxes: forward_terminate → engine.terminate →
    // remove_remote → destroy. The destroy button is always visible regardless of deploy state.
    await row.destroy();

    assert(
      !podmanOk(LOCAL_CONNECTION, ["inspect", projectName]),
      `Local container "${projectName}" still exists on ${LOCAL_CONNECTION} after destroy.`
    );
    assert(
      !podmanOk(REMOTE_CONNECTION, ["inspect", projectName]),
      `Remote container "${projectName}" still exists on ${REMOTE_CONNECTION} after destroy. ` +
        "destroy_sandbox must call store.remove_remote before store.destroy."
    );

    // destroy_sandbox must call sync::forward_terminate for the deployed-sandbox path.
    const fwdOut = forwardList(`terminal-${projectName}`);
    assert(
      !fwdOut.includes("Connected"),
      `Mutagen forwarding session terminal-${projectName} still shows Connected after destroy. Output:\n${fwdOut}\n` +
        "destroy_sandbox must call sync::forward_terminate before engine.terminate in the deployed branch."
    );
  });
});
