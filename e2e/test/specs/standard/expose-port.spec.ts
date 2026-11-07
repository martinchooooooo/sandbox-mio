import assert from "node:assert";
import net from "node:net";
import fs from "fs";
import path from "path";

import ProjectsPage from "../../pageobjects/projects.page.js";
import workspaceDashboard from "../../pageobjects/workspace-dashboard.page.js";
import { podman, podmanOk, LOCAL_CONNECTION } from "../../../lib/podman-connection.js";
import { forwardList, forwardTerminate } from "../../../lib/mutagen.js";
import { captureDiagOnFailure } from "../../../lib/diag.js";
import { useSandboxWorkspace } from "../../../lib/sandbox-suite.js";
import { COMMAND, SUBPROCESS } from "../../../lib/timeouts.js";

// Two distinct sandbox-side ports: one exposed manually via the "Expose a port" UI, one declared
// through `.mio/ports.json` to exercise the `watch_ports_file` auto-expose/auto-unexpose path.
const MANUAL_PORT = 4000;
const FILE_PORT = 5000;
const UPGRADE_PORT = 4001;
const MANUAL_TOKEN = "manual-echo";
const FILE_TOKEN = "file-echo";
const UPGRADE_TOKEN = "upgrade-echo";

// Starts a throwaway TCP "service" inside the sandbox with socat (apt-installed in
// src-tauri/resources/Containerfile): on every connection it writes `token` and closes. This stands in for a user's own dev server bound to `target_port` inside the container.
function startFakeService(sandbox: string, targetPort: number, token: string): void {
  podman(LOCAL_CONNECTION, [
    "exec",
    "-d",
    sandbox,
    "socat",
    `TCP-LISTEN:${targetPort},fork,reuseaddr`,
    `SYSTEM:echo -n ${token}`,
  ]);
}

// Dials 127.0.0.1:hostPort (the Windows-host loopback port Mutagen forwards) and returns whatever
// bytes the fake service wrote before closing. This is the real path a browser would take to reach
// an exposed port — through the Mutagen forward into the container's own loopback.
function readOnce(hostPort: number, timeoutMs = 10000): Promise<string> {
  return new Promise((resolve, reject) => {
    const socket = net.createConnection({ host: "127.0.0.1", port: hostPort });
    const chunks: Buffer[] = [];
    const timer = setTimeout(() => {
      socket.destroy();
      reject(new Error(`no data read from 127.0.0.1:${hostPort} within ${timeoutMs}ms`));
    }, timeoutMs);
    socket.on("data", (d) => chunks.push(d));
    socket.on("close", () => {
      clearTimeout(timer);
      resolve(Buffer.concat(chunks).toString("utf8"));
    });
    socket.on("error", (e) => {
      clearTimeout(timer);
      reject(e);
    });
  });
}

describe("Exposing sandbox ports (network::expose_port / unexpose_port / watch_ports_file)", function () {
  this.timeout(300000);

  const { projectName, row, workspaceDir } = useSandboxWorkspace("e2e-exposeport");
  const portsJsonPath = path.join(workspaceDir, ".mio", "ports.json");

  after(() => {
    forwardTerminate(`expose-${projectName}-${MANUAL_PORT}`);
    forwardTerminate(`expose-${projectName}-${FILE_PORT}`);
    forwardTerminate(`expose-${projectName}-${UPGRADE_PORT}`);
  });

  captureDiagOnFailure();

  it("creates and starts a local sandbox, then opens its dashboard", async () => {
    await ProjectsPage.open();
    await ProjectsPage.createProject(projectName);
    await row.start();
    await row.waitForRunning();

    await row.openWorkspace();
    await workspaceDashboard.waitForRendered(projectName);
    await (await workspaceDashboard.noPortsText).waitForDisplayed({ timeout: COMMAND });
  });

  it("exposes a port via the UI and reaches the real service through the forward", async () => {
    startFakeService(projectName, MANUAL_PORT, MANUAL_TOKEN);

    await workspaceDashboard.exposePort(MANUAL_PORT);
    const label = `Exposed :${MANUAL_PORT}`;
    await workspaceDashboard.waitForPortRow(label);

    assert(
      await workspaceDashboard.hasCloseButton(label),
      `manually-exposed row "${label}" should show a "Stop exposing" button`
    );

    const hostPort = await workspaceDashboard.portRowHostPort(label);
    const fwdOut = forwardList(`expose-${projectName}-${MANUAL_PORT}`);
    assert(
      fwdOut.includes("Connected"),
      `mutagen forward expose-${projectName}-${MANUAL_PORT} did not show Connected. Output:\n${fwdOut}`
    );

    const received = await readOnce(hostPort);
    assert.strictEqual(
      received,
      MANUAL_TOKEN,
      `Dialing the exposed host port ${hostPort} did not reach the fake service on container port ${MANUAL_PORT}. ` +
        `Got: "${received}". This can happen when the forward ` +
        "pointed at the wrong remote port."
    );

    assert(
      fs.existsSync(portsJsonPath),
      `expose_port must bootstrap ${portsJsonPath} on first expose (ensure_ports_file, src-tauri/src/network/mod.rs)`
    );
  });

  it("an explicit expose upgrades a port the listening-socket scan already auto-detected", async () => {
    // Regression coverage for the race documented in docs/features/expose-ports/04_improvements.md:
    // start the service (and let the scan claim it as `Auto :`) *before* exposing it via the UI, so
    // the manual expose_port call always finds the port already tracked as auto_detected.
    startFakeService(projectName, UPGRADE_PORT, UPGRADE_TOKEN);

    const autoLabel = `Auto :${UPGRADE_PORT}`;
    await workspaceDashboard.waitForPortRow(autoLabel, SUBPROCESS);
    assert(
      !(await workspaceDashboard.hasCloseButton(autoLabel)),
      `scan-claimed row "${autoLabel}" should not show a "Stop exposing" button before the manual upgrade`
    );

    await workspaceDashboard.exposePort(UPGRADE_PORT);
    const exposedLabel = `Exposed :${UPGRADE_PORT}`;
    await workspaceDashboard.waitForPortRow(exposedLabel);
    await workspaceDashboard.waitForPortRowGone(autoLabel);

    assert(
      await workspaceDashboard.hasCloseButton(exposedLabel),
      `upgraded row "${exposedLabel}" should show a "Stop exposing" button after the manual expose wins ownership`
    );

    const hostPort = await workspaceDashboard.portRowHostPort(exposedLabel);
    const received = await readOnce(hostPort);
    assert.strictEqual(
      received,
      UPGRADE_TOKEN,
      `Dialing the upgraded host port ${hostPort} did not reach the fake service on container port ${UPGRADE_PORT}. Got: "${received}".`
    );
  });

  it("declaring a port in .mio/ports.json auto-exposes it (watch_ports_file)", async () => {
    startFakeService(projectName, FILE_PORT, FILE_TOKEN);

    const existing = JSON.parse(fs.readFileSync(portsJsonPath, "utf8")) as Record<string, string>;
    existing[String(FILE_PORT)] = "webapp";
    fs.writeFileSync(portsJsonPath, JSON.stringify(existing));

    // Beyond poll_ports_file's 2s tick (PORTS_FILE_POLL_INTERVAL, src-tauri/src/network/mod.rs),
    // the noticing tick still has to stand up a Mutagen forward.
    await workspaceDashboard.waitForPortRow("webapp", SUBPROCESS);

    assert(
      !(await workspaceDashboard.hasCloseButton("webapp")),
      '"webapp" row is file-declared and must not show a manual "Stop exposing" button'
    );

    const hostPort = await workspaceDashboard.portRowHostPort("webapp");
    const received = await readOnce(hostPort);
    assert.strictEqual(
      received,
      FILE_TOKEN,
      `Dialing the auto-exposed host port ${hostPort} did not reach the fake service on container port ${FILE_PORT}. Got: "${received}".`
    );
  });

  it("removing the ports.json entry auto-unexposes it", async () => {
    const existing = JSON.parse(fs.readFileSync(portsJsonPath, "utf8")) as Record<string, string>;
    delete existing[String(FILE_PORT)];
    fs.writeFileSync(portsJsonPath, JSON.stringify(existing));

    await workspaceDashboard.waitForPortRowGone("webapp", SUBPROCESS);

    await browser.waitUntil(() => !forwardList(`expose-${projectName}-${FILE_PORT}`).includes("Connected"), {
      timeout: SUBPROCESS,
      interval: 1000,
      timeoutMsg: `mutagen forward expose-${projectName}-${FILE_PORT} is still Connected after removing its ports.json entry`,
    });
  });

  it("clicking the close button unexposes the manually-opened port", async () => {
    const label = `Exposed :${MANUAL_PORT}`;
    await workspaceDashboard.closePortRow(label);
    await workspaceDashboard.waitForPortRowGone(label);

    await browser.waitUntil(() => !forwardList(`expose-${projectName}-${MANUAL_PORT}`).includes("Connected"), {
      timeout: SUBPROCESS,
      interval: 1000,
      timeoutMsg: `mutagen forward expose-${projectName}-${MANUAL_PORT} is still Connected after clicking its close button`,
    });

    // No "No ports open." assertion here: unexpose_port only drops the tracked forward, it doesn't
    // suppress re-detection, so the still-listening fake service comes back as `Auto :` on the next
    // poll_ports_file tick. See docs/features/expose-ports/04_improvements.md.
  });

  it("destroy removes the container and its forwards stay gone", async () => {
    await row.destroy();
    assert(
      !podmanOk(LOCAL_CONNECTION, ["inspect", projectName]),
      `Local container "${projectName}" still exists after destroy.`
    );
    const fwdOut = forwardList(`expose-${projectName}-${MANUAL_PORT}`);
    assert(
      !fwdOut.includes("Connected"),
      `Mutagen forward expose-${projectName}-${MANUAL_PORT} still Connected after destroy. Output:\n${fwdOut}`
    );
  });
});
