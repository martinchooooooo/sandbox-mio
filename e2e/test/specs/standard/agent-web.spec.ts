import assert from "node:assert";
import fs from "fs";
import path from "path";

import ProjectsPage from "../../pageobjects/projects.page.js";
import workspaceDashboard from "../../pageobjects/workspace-dashboard.page.js";
import { useDashboardWebview } from "../../../lib/dashboard-suite.js";
import { useSandboxWorkspace } from "../../../lib/sandbox-suite.js";
import { podman, podmanOk, LOCAL_CONNECTION } from "../../../lib/podman-connection.js";
import { forwardList } from "../../../lib/mutagen.js";
import {
  workspaceState,
  panesFor,
  paneFor,
  openAgentWeb,
  closeWorkspace,
  switchPane,
} from "../../../lib/workspace.js";

const PASEO_WEB_PORT = "6767";

// Normalize a host path for equality: the Rust registry stores whatever separators the frontend
// passed, so compare resolved, lower-cased forms.
function samePath(a: string, b: string): boolean {
  return path.resolve(a).toLowerCase() === path.resolve(b).toLowerCase();
}

// Authenticated status probe — the exact request fetch_paseo_server_info (src-tauri/src/lib.rs)
// must have already succeeded at for the pane to exist. Returns the serverId on HTTP 200, or
// undefined on any failure.
async function fetchServerId(port: string, password: string): Promise<string | undefined> {
  try {
    const res = await fetch(`http://127.0.0.1:${port}/api/status`, {
      headers: { Authorization: `Bearer ${password}` },
    });
    if (res.status !== 200) return undefined;
    const body = (await res.json()) as { serverId?: unknown };
    return typeof body.serverId === "string" && body.serverId.length > 0 ? body.serverId : undefined;
  } catch {
    return undefined;
  }
}

describe("Agent web UI (paseo)", function () {
  this.timeout(900000);

  const { projectName, row, workspaceDir } = useSandboxWorkspace("e2e-agentweb");
  // prepare_agent_web_pane persists the minted password host-side here (next to zellij.json).
  const passwordPath = path.join(workspaceDir, ".mio", "paseo.json");
  let serverId: string;
  let password: string;

  useDashboardWebview();

  it("creates a local sandbox with the paseo port published", async () => {
    await ProjectsPage.open();
    await ProjectsPage.createProject(projectName, { timeout: 600000 });

    const bindings = podman(LOCAL_CONNECTION, [
      "inspect",
      projectName,
      "--format",
      `{{json (index .HostConfig.PortBindings "${PASEO_WEB_PORT}/tcp")}}`,
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
      `Container "${projectName}" has no 127.0.0.1 binding for port ${PASEO_WEB_PORT}. Output:\n${bindings}\n` +
        "This can happen when create did not pass --publish 127.0.0.1::6767 (agent-web-ui Phase A) " +
        "or the sandbox was created from base image version 2."
    );
  });

  it("starts the sandbox", async () => {
    await row.start();
    await row.waitForRunning();
    await expect(row.statusText).toHaveText(/running/);
  });

  it("opens the workspace with only a Dashboard pane — no AgentWeb pane yet, no Terminal pane yet", async () => {
    await row.openWorkspace();

    // open_workspace registers the pane asynchronously; poll until it exists.
    await browser.waitUntil(
      async () => panesFor(await workspaceState(), projectName).length === 1,
      {
        timeout: 60000,
        interval: 1000,
        timeoutMsg: `sandbox "${projectName}" never registered its Dashboard pane`,
      }
    );

    const state = await workspaceState();
    const panes = panesFor(state, projectName);
    assert.deepStrictEqual(
      panes.map((p) => p.kind),
      ["dashboard"],
      `Expected exactly a Dashboard pane, got: ${JSON.stringify(panes.map((p) => p.label))}`
    );
    assert(
      samePath(panes[0].workspace_path, workspaceDir),
      `Pane ${panes[0].label} carries workspace_path "${panes[0].workspace_path}", expected "${workspaceDir}". ` +
        "The dashboard launcher needs it to invoke open_agent_web without a sandbox lookup."
    );
  });

  it("clicking the Manage tab's Agents launcher button creates the AgentWeb pane (paseo install + daemon + forward)", async function () {
    await workspaceDashboard.switchToDashboard(projectName);
    await workspaceDashboard.launchApp("agentweb");

    await browser.waitUntil(
      async () => paneFor(await workspaceState(), projectName, "agentweb") !== undefined,
      {
        timeout: 840000,
        interval: 3000,
        timeoutMsg:
          "no agentweb pane appeared after clicking the Agents launcher button. This can happen " +
          "when open_agent_web failed (check AgentWebUnavailable in the app log) or the click " +
          "did not reach the dashboard webview.",
      }
    );

    const state = await workspaceState();
    const pane = paneFor(state, projectName, "agentweb")!;
    assert.strictEqual(pane.label, `workspace-${projectName}-agentweb`);
    assert.strictEqual(pane.forward_name, `agentweb-${projectName}`);
    // Lazy embed activates immediately (embed_agent_web, workspace/mod.rs).
    assert.strictEqual(
      state.active_app,
      pane.label,
      `active_app is ${state.active_app}, expected the new agentweb pane to activate on creation.`
    );

    // The minted password is persisted host-side (mint-once contract; step 6 asserts reuse).
    assert(fs.existsSync(passwordPath), `password file ${passwordPath} does not exist`);
    const session = JSON.parse(fs.readFileSync(passwordPath, "utf8"));
    assert(
      typeof session.password === "string" && session.password.length > 0,
      `Persisted ${passwordPath} has no password. Contents:\n${JSON.stringify(session)}`
    );
    password = session.password;

    const manifest = fs.readFileSync(path.join(workspaceDir, "mise.toml"), "utf8");
    assert(manifest.includes("node"), `mise.toml does not list "node". Contents:\n${manifest}`);
    assert(
      manifest.includes("npm:@getpaseo/cli"),
      `mise.toml does not list npm:@getpaseo/cli. Contents:\n${manifest}\n` +
        "An image-baked paseo would leave the manifest clean and silently break remote deploys."
    );

    // The daemon is running *and* procps is baked (ps itself is the Phase A dependency: paseo's
    // `daemon stop` shells out to it).
    const procs = podman(LOCAL_CONNECTION, ["exec", projectName, "ps", "aux"], { check: false });
    assert(
      procs.includes("paseo"),
      `"ps aux" in the container shows no paseo process. Output:\n${procs}\n` +
        "This can happen when the daemon start failed or the image lacks procps (base image v2)."
    );

    const fwdOut = forwardList(`agentweb-${projectName}`);
    assert(
      fwdOut.includes("Connected"),
      `mutagen forward list agentweb-${projectName} did not show Connected. Output:\n${fwdOut}\n` +
        "prepare_agent_web_pane must create the forward before fetching /api/status."
    );
  });

  it("the forwarded port serves /api/status with the persisted password", async () => {
    const pane = paneFor(await workspaceState(), projectName, "agentweb")!;
    const port = new URL(pane.url).port;
    assert(port, `agentweb pane url "${pane.url}" has no port`);

    let id: string | undefined;
    await browser.waitUntil(
      async () => {
        id = await fetchServerId(port, password);
        return id !== undefined;
      },
      {
        timeout: 30000,
        interval: 1000,
        timeoutMsg:
          `http://127.0.0.1:${port}/api/status never returned 200 with a serverId using the ` +
          "persisted password. This can happen when the forward died or the daemon rejected the password.",
      }
    );
    serverId = id!;
  });

  it("re-opening is switch-not-duplicate and keeps the password", async () => {
    // Terminal is never opened in this spec (it's a separate lazy pane); switch off Agents onto
    // the always-present Dashboard pane so the re-open below has something to switch away from.
    await switchPane(projectName, "dashboard");
    await openAgentWeb(projectName, workspaceDir);

    const state = await workspaceState();
    const agentPanes = panesFor(state, projectName).filter((p) => p.kind === "agentweb");
    assert.strictEqual(
      agentPanes.length,
      1,
      `Expected exactly one agentweb pane after re-open, got ${agentPanes.length} — ` +
        "the open_agent_web exists-guard must switch, not duplicate."
    );
    assert.strictEqual(
      state.active_app,
      agentPanes[0].label,
      "Re-open did not switch back to the agentweb pane."
    );

    // Mint-once, reuse-on-restart: the persisted password must be stable across opens.
    const session = JSON.parse(fs.readFileSync(passwordPath, "utf8"));
    assert.strictEqual(
      session.password,
      password,
      "The persisted paseo password changed on re-open; open_agent_web must reuse it, not re-mint."
    );

    // The daemon still answers on the pane's current URL with the same password and identity.
    const port = new URL(agentPanes[0].url).port;
    const id = await fetchServerId(port, password);
    assert(id, `/api/status on port ${port} did not return 200 with a serverId after re-open`);
    assert.strictEqual(id, serverId, "The daemon's serverId changed across a pane re-open.");
  });

  it("closing the workspace tears down the forward but leaves the daemon running", async () => {
    await closeWorkspace(projectName);

    await browser.waitUntil(
      () => !forwardList(`agentweb-${projectName}`).includes("Connected"),
      {
        timeout: 30000,
        interval: 1000,
        timeoutMsg:
          `Mutagen forward agentweb-${projectName} still shows Connected after close_workspace. ` +
          "close_app must terminate AgentWeb forwards like Terminal ones.",
      }
    );

    // The daemon outlives the pane (Lifecycle: re-opening later must be instant).
    const procs = podman(LOCAL_CONNECTION, ["exec", projectName, "ps", "aux"], { check: false });
    assert(
      procs.includes("paseo"),
      `paseo daemon is gone after close_workspace. ps aux:\n${procs}\n` +
        "Closing the pane must only tear down the forward, never stop the daemon."
    );
  });

  it("destroy removes the container and the forward stays gone", async () => {
    await row.destroy();
    assert(
      !podmanOk(LOCAL_CONNECTION, ["inspect", projectName]),
      `Local container "${projectName}" still exists after destroy.`
    );
    // destroy_sandbox terminates agentweb- forwards explicitly even when no pane is open.
    const fwdOut = forwardList(`agentweb-${projectName}`);
    assert(
      !fwdOut.includes("Connected"),
      `Mutagen forward agentweb-${projectName} still Connected after destroy. Output:\n${fwdOut}`
    );
  });
});
