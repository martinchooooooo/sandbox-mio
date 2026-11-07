import assert from "node:assert";
import fs from "fs";
import path from "path";

import ProjectsPage from "../../pageobjects/projects.page.js";
import workspaceDashboard from "../../pageobjects/workspace-dashboard.page.js";
import { captureDiagOnFailure } from "../../../lib/diag.js";
import { useSandboxWorkspace } from "../../../lib/sandbox-suite.js";
import { podman, podmanOk, LOCAL_CONNECTION } from "../../../lib/podman-connection.js";
import { forwardList } from "../../../lib/mutagen.js";
import { MANAGED_SSH_CONFIG, sshAliasBlock } from "../../../lib/ssh.js";
import { APP_DATA_ROOT } from "../../../lib/run-identity.js";
import {
  workspaceState,
  panesFor,
  paneFor,
  openAgentWeb,
  closeWorkspace,
  hideActivePane,
} from "../../../lib/workspace.js";

// Mirrors the auth check fetch_paseo_server_info makes (src-tauri/src/lib.rs) before a pane exists.
// Returns the serverId on HTTP 200, undefined otherwise.
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
  // prepare_agent_web_pane persists the minted password host-side in app data, never in the synced
  // workspace (src-tauri/src/paths.rs `sandbox_pane_dir`).
  const passwordPath = path.join(APP_DATA_ROOT, "panes", projectName, "paseo.json");
  const keyDir = path.join(APP_DATA_ROOT, "ssh-keys", projectName);
  let serverId: string;
  let password: string;

  captureDiagOnFailure();

  it("creates a local sandbox that publishes nothing and gets an exec-transport SSH alias", async () => {
    await ProjectsPage.open();
    await ProjectsPage.createProject(projectName, { timeout: 600000 });

    // An exec-capable sandbox publishes no ports at all — paseo is reached through the
    // `podman exec`-tunneled sshd instead (docs/features/podman-exec-transport/02_dev.md).
    const bindings = podman(LOCAL_CONNECTION, [
      "inspect",
      projectName,
      "--format",
      "{{json .HostConfig.PortBindings}}",
    ]).trim();
    assert(
      bindings === "null" || bindings === "{}",
      `Container "${projectName}" published ports: ${bindings}\n` +
        "create must pass no --publish for a sandbox on the exec transport."
    );

    const block = sshAliasBlock(projectName);
    assert(
      block !== undefined,
      `No "Host ${projectName}" block in ${MANAGED_SSH_CONFIG}. ` +
        "create must call ensure_sandbox_alias for an exec-capable sandbox."
    );
    for (const keyword of ["ProxyCommand", "User root", "IdentityFile", "UserKnownHostsFile"]) {
      assert(
        block.includes(keyword),
        `The "Host ${projectName}" alias has no ${keyword}. Block:\n${block}`
      );
    }
  });

  it("starts the sandbox with the per-sandbox public key installed in the image's sshd", async () => {
    await row.start();
    await row.waitForRunning();
    await expect(row.statusText).toHaveText(/running/);

    // install_sandbox_key + the Containerfile's baked config are the two halves of the handshake
    // every later step's forward depends on; asserting them here localises a failure.
    const authorized = podman(LOCAL_CONNECTION, [
      "exec",
      projectName,
      "cat",
      "/etc/ssh/authorized_keys.container",
    ]);
    assert(
      authorized.includes("ssh-ed25519"),
      `/etc/ssh/authorized_keys.container holds no ed25519 key. Contents:\n${authorized}\n` +
        "install_sandbox_key must podman cp the sandbox's public key after create."
    );
    const sshdConfig = podman(LOCAL_CONNECTION, [
      "exec",
      projectName,
      "cat",
      "/etc/ssh/sshd_config.container",
    ]);
    assert(
      sshdConfig.includes("AuthorizedKeysFile /etc/ssh/authorized_keys.container"),
      `/etc/ssh/sshd_config.container is missing or wrong. Contents:\n${sshdConfig}\n` +
        "This can happen when the sandbox was built from a base image older than " +
        "EXEC_TRANSPORT_MIN_IMAGE_VERSION."
    );
  });

  it("opens the workspace showing only the Dashboard — no pane registered yet, no Terminal pane yet", async () => {
    await row.openWorkspace();
    await workspaceDashboard.waitForRendered(projectName);

    const state = await workspaceState();
    assert.strictEqual(
      panesFor(state, projectName).length,
      0,
      "No AppPane should exist yet — Dashboard is client-side view state, not a Rust-tracked pane " +
        "(docs/features/consolidate-app-container/02_dev.md Resolved Decision 2).",
    );
    assert.strictEqual(state.active_app, null);
  });

  it("installs the Pi agent so the Manage tab's Agents launcher becomes available", async () => {
    // LAUNCHABLE_APPS gates "agentweb" on installedAgents.length > 0 (src/lib/workspace/apps.ts,
    // docs/features/workspace-app-tabs/02_dev.md Decision 1) — the launcher does not render until an
    // agent exists. Pi is installed purely to satisfy that gate (lightest catalogue entry, no auth flow).
    await workspaceDashboard.startAgentInstall("Pi");
    await workspaceDashboard.waitForAgentRow("Pi", 300000);
  });

  it("clicking the Manage tab's Agents launcher button creates the AgentWeb pane (paseo install + daemon + forward)", async function () {
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
    assert(
      !fs.existsSync(path.join(workspaceDir, ".mio", "paseo.json")),
      "paseo.json was written into the synced workspace; the password belongs in app data only."
    );
    assert(fs.existsSync(keyDir), `ssh identity dir ${keyDir} does not exist before destroy`);
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
    // Terminal is never opened in this spec (it's a separate lazy pane); hide the AgentWeb overlay
    // and fall back to Dashboard so the re-open below has something to switch away from.
    await hideActivePane();
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

  it("destroy also removes the sandbox's ssh alias, identity directory and pane state", () => {
    assert.strictEqual(
      sshAliasBlock(projectName),
      undefined,
      `"Host ${projectName}" block still in ${MANAGED_SSH_CONFIG} after destroy. ` +
        "destroy must call cleanup_host_state (src-tauri/src/services/sandbox.rs)."
    );
    assert(!fs.existsSync(keyDir), `ssh identity dir ${keyDir} still exists after destroy.`);
    assert(
      !fs.existsSync(path.dirname(passwordPath)),
      `pane state dir ${path.dirname(passwordPath)} still exists after destroy.`
    );
  });
});
