import assert from "node:assert";
import net from "node:net";
import fs from "fs";
import path from "path";

import ProjectsPage from "../../pageobjects/projects.page.js";
import workspaceDashboard from "../../pageobjects/workspace-dashboard.page.js";
import { podman, podmanOk, LOCAL_CONNECTION } from "../../../lib/podman-connection.js";
import { forwardList, forwardTerminate } from "../../../lib/mutagen.js";
import { useDashboardWebview, dashboardIt } from "../../../lib/dashboard-suite.js";
import { useSandboxWorkspace } from "../../../lib/sandbox-suite.js";

// Two distinct sandbox-side ports: one exposed manually via the "Expose a port" UI, one declared
// through `.mio/ports.json` to exercise the `watch_ports_file` auto-expose/auto-unexpose path.
const MANUAL_PORT = 4000;
const FILE_PORT = 5000;
const MANUAL_TOKEN = "manual-echo";
const FILE_TOKEN = "file-echo";

// Starts a throwaway TCP "service" inside the sandbox with socat (already apt-installed for the
// relay itself, see src-tauri/resources/Containerfile): on every connection it writes `token` and
// closes. This stands in for a user's own dev server bound to `target_port` inside the container.
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
// an exposed port — through the relay-pool socat inside the container and back over the forward.
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
  });

  useDashboardWebview();

  it("creates and starts a local sandbox, then opens its dashboard", async () => {
    await ProjectsPage.open();
    await ProjectsPage.createProject(projectName);
    await row.start();
    await row.waitForRunning();

    await row.openWorkspace();
    await workspaceDashboard.switchToDashboard(projectName);
    await (await workspaceDashboard.noPortsText).waitForDisplayed({ timeout: 10000 });
  });

  dashboardIt("exposes a port via the UI and reaches the real service through the forward", () => projectName, async () => {
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
        `Got: "${received}". This can happen when the relay-pool socat bridged the wrong slot or the forward ` +
        "pointed at the wrong remote port."
    );

    assert(
      fs.existsSync(portsJsonPath),
      `expose_port must bootstrap ${portsJsonPath} on first expose (ensure_ports_file, src-tauri/src/network/mod.rs)`
    );
  });

  dashboardIt("declaring a port in .mio/ports.json auto-exposes it (watch_ports_file)", () => projectName, async () => {
    startFakeService(projectName, FILE_PORT, FILE_TOKEN);

    const existing = JSON.parse(fs.readFileSync(portsJsonPath, "utf8")) as Record<string, string>;
    existing[String(FILE_PORT)] = "webapp";
    fs.writeFileSync(portsJsonPath, JSON.stringify(existing));

    // poll_ports_file ticks every 2s (PORTS_FILE_POLL_INTERVAL, src-tauri/src/network/mod.rs).
    await workspaceDashboard.waitForPortRow("webapp", 15000);

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

  dashboardIt("removing the ports.json entry auto-unexposes it", () => projectName, async () => {
    const existing = JSON.parse(fs.readFileSync(portsJsonPath, "utf8")) as Record<string, string>;
    delete existing[String(FILE_PORT)];
    fs.writeFileSync(portsJsonPath, JSON.stringify(existing));

    await workspaceDashboard.waitForPortRowGone("webapp", 15000);

    await browser.waitUntil(() => !forwardList(`expose-${projectName}-${FILE_PORT}`).includes("Connected"), {
      timeout: 15000,
      interval: 1000,
      timeoutMsg: `mutagen forward expose-${projectName}-${FILE_PORT} is still Connected after removing its ports.json entry`,
    });
  });

  dashboardIt("clicking the close button unexposes the manually-opened port", () => projectName, async () => {
    const label = `Exposed :${MANUAL_PORT}`;
    await workspaceDashboard.closePortRow(label);
    await workspaceDashboard.waitForPortRowGone(label);

    await browser.waitUntil(() => !forwardList(`expose-${projectName}-${MANUAL_PORT}`).includes("Connected"), {
      timeout: 15000,
      interval: 1000,
      timeoutMsg: `mutagen forward expose-${projectName}-${MANUAL_PORT} is still Connected after clicking its close button`,
    });

    await (await workspaceDashboard.noPortsText).waitForDisplayed({ timeout: 10000 });
  });

  it("destroy removes the container and its forwards stay gone", async () => {
    await workspaceDashboard.switchToAppWebview();
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
