import assert from "node:assert";
import fs from "fs";
import path from "path";

import ProjectsPage from "../pageobjects/projects.page.js";
import ProjectRow from "../pageobjects/project-row.component.js";
import { podman, podmanOk, LOCAL_CONNECTION } from "../../lib/podman-connection.js";

const WORKSPACE_ROOT = process.env.SANDBOX_WORKSPACE_ROOT;

// Container port zellij's web server binds; matches ZELLIJ_WEB_PORT in both src-tauri crates.
const ZELLIJ_WEB_PORT = "8082";

// Parse the host loopback port from `podman port <name> 8082/tcp` output (one `<ip>:<port>` line
// per mapping). Mirrors parse_host_port in src-tauri/src/runtime/mod.rs.
function hostPortFrom(output: string): string | undefined {
  for (const line of output.split(/\r?\n/)) {
    const m = line.trim().match(/^127\.0\.0\.1:(\d+)$/);
    if (m) return m[1];
  }
  return undefined;
}

// `function ()` (not an arrow) so `this.timeout` raises the suite ceiling: the first create on a
// connection may trigger the one-time mise base-image build (~324 MB).
describe("In-app web terminal (zellij)", function () {
  this.timeout(600000);

  const projectName = `e2e-terminal-${Date.now()}`;
  const row: ProjectRow = ProjectsPage.row(projectName);
  let workspaceDir: string;
  // open_terminal persists the token host-side here (the docs/techdebt.md workaround).
  let tokenPath: string;

  before(() => {
    expect(WORKSPACE_ROOT).toBeTruthy();
    workspaceDir = path.join(WORKSPACE_ROOT!, projectName);
    tokenPath = path.join(workspaceDir, ".mio", "zellij.json");
  });

  after(() => {
    // Best-effort cleanup so a failed run leaves no container behind.
    podman(LOCAL_CONNECTION, ["rm", "-f", projectName], { check: false });
  });

  // Capture the webview state on any failure (URL + page source + screenshot to e2e/.diag),
  // mirroring runtime-management.spec.ts.
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

  it("creates a local sandbox with the web port published", async () => {
    await ProjectsPage.open();
    // The wide wait covers a possible one-time base-image build. A create that rejected the new
    // --publish/--network flags would never show a row, failing here.
    await ProjectsPage.createProject(projectName, { timeout: 600000 });

    // Read the *configured* binding via inspect, not `podman port`: under rootless pasta
    // networking the host forward is only materialized once the container is running, so
    // `podman port` reports nothing for this freshly-created-but-not-started sandbox. The
    // create-time --publish 127.0.0.1::8082 (Phase B) shows up in .HostConfig.PortBindings
    // regardless of run state.
    const bindings = podman(LOCAL_CONNECTION, [
      "inspect",
      projectName,
      "--format",
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
      `Container "${projectName}" has no 127.0.0.1 binding for port ${ZELLIJ_WEB_PORT}. Output:\n${bindings}\n` +
        "This can happen when create did not pass --publish 127.0.0.1::8082 (Phase B)."
    );
  });

  it("starts the sandbox", async () => {
    await row.start();
    await row.waitForRunning();
    await expect(row.statusText).toHaveText(/running/);
  });

  it("opens the terminal: server online, token persisted, port serves HTTP 200", async () => {
    await row.openTerminal();

    // The token file is written host-side once open_terminal succeeds; poll for it (the command
    // does a zellij daemonize + token mint, which is not instant).
    await browser.waitUntil(() => fs.existsSync(tokenPath), {
      timeout: 60000,
      interval: 1000,
      timeoutMsg: `token file ${tokenPath} did not appear after openTerminal`,
    });

    // The zellij web server is online inside the container — proves zellij is on the global PATH
    // from the baked image (Phase A), reached without mise (no `mise exec`/activation). `--status`
    // exits non-zero when offline, so read with check:false; "online" is not a substring of "offline".
    const status = podman(LOCAL_CONNECTION, ["exec", projectName, "zellij", "web", "--status"], {
      check: false,
    });
    assert(
      status.includes("online"),
      `zellij web --status reported "${status}", expected it to contain "online". ` +
        "This can happen when the baked image lacks zellij or the config file is missing (Phase A)."
    );

    // The persisted token is a non-empty value and the URL points at the published loopback port.
    const session = JSON.parse(fs.readFileSync(tokenPath, "utf8"));
    assert(
      typeof session.token === "string" && session.token.length > 0,
      `Persisted ${tokenPath} has no token. Contents:\n${JSON.stringify(session)}\n` +
        "This can happen when the token parser in open_terminal did not match the --create-token output."
    );

    const portOut = podman(LOCAL_CONNECTION, ["port", projectName, `${ZELLIJ_WEB_PORT}/tcp`]);
    const hostPort = hostPortFrom(portOut);
    assert(hostPort !== undefined, `No published host port for ${projectName}. Output:\n${portOut}`);
    assert(
      session.url === `http://127.0.0.1:${hostPort}/`,
      `Persisted url "${session.url}" does not match the published port http://127.0.0.1:${hostPort}/. ` +
        "open_terminal must report the port it actually published."
    );

    // The decisive end-to-end proof: the published host-loopback port serves the web client. Poll
    // because the server may take a moment to bind after daemonize.
    let status200 = false;
    await browser.waitUntil(
      async () => {
        try {
          const res = await fetch(session.url);
          status200 = res.status === 200;
          return status200;
        } catch {
          return false;
        }
      },
      {
        timeout: 30000,
        interval: 1000,
        timeoutMsg: `${session.url} never returned HTTP 200 from the host. ` +
          "This can happen when pasta did not forward host-loopback to container-loopback (--host-lo-to-ns-lo).",
      }
    );
    assert(status200, `${session.url} did not serve HTTP 200`);
  });

  it("re-opening the terminal stays online and still serves HTTP 200 (idempotent)", async () => {
    await row.openTerminal();

    const status = podman(LOCAL_CONNECTION, ["exec", projectName, "zellij", "web", "--status"], {
      check: false,
    });
    assert(status.includes("online"), `zellij web --status reported "${status}" on re-open.`);

    const session = JSON.parse(fs.readFileSync(tokenPath, "utf8"));
    const res = await fetch(session.url);
    assert(
      res.status === 200,
      `${session.url} returned ${res.status} on re-open, expected 200. ` +
        "Re-opening must re-attach to the running server, not break it."
    );
  });

  it("destroy removes the container", async () => {
    await row.destroy();
    assert(
      !podmanOk(LOCAL_CONNECTION, ["inspect", projectName]),
      `Local container "${projectName}" still exists after destroy.`
    );
  });
});
