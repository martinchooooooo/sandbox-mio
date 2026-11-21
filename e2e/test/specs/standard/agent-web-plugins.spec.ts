import assert from "node:assert";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

import ProjectsPage from "../../pageobjects/projects.page.js";
import { captureDiagOnFailure } from "../../../lib/diag.js";
import { useSandboxWorkspace } from "../../../lib/sandbox-suite.js";
import { podman, podmanOk, LOCAL_CONNECTION } from "../../../lib/podman-connection.js";
import { WORKTREE_ROOT } from "../../../lib/run-identity.js";
import { AGENT_SETTLE } from "../../../lib/timeouts.js";
import { workspaceState, paneFor, openAgentWeb, closePane } from "../../../lib/workspace.js";

// STANDARD spec for docs/features/paseo-plugins: the Agents-tab open delivers the app-owned plugins
// into a real sandbox, registers them with the real Paseo daemon, and applies later changes by
// reload (contents) or restart (plugin set). State is changed inside the container, never in the
// repository's paseo-plugins/, so a neighbouring edit cannot race the run.
const PLUGIN_ID = "mio-probe";
const PLUGIN_DIR = "/usr/local/lib/mio/paseo-plugins";
const HOST_PLUGIN_DIR = path.join(WORKTREE_ROOT, "paseo-plugins", PLUGIN_ID);

describe("Paseo plugins delivery and registration", function () {
  this.timeout(900000);

  const { projectName, row, workspaceDir } = useSandboxWorkspace("e2e-plugins");

  captureDiagOnFailure();

  const exec = (...args: string[]): string => podman(LOCAL_CONNECTION, ["exec", projectName, ...args]);
  const sh = (script: string): string => exec("sh", "-c", script);
  const paseo = (...args: string[]): string =>
    podman(LOCAL_CONNECTION, [
      "exec", "--workdir", "/root/work", projectName, "mise", "exec", "--", "paseo", ...args,
    ], { check: false });
  const daemonPid = (): number => JSON.parse(sh("cat /root/.paseo/paseo.pid")).pid;
  const readConfig = (): { pluginsEnabled?: boolean; plugins?: Record<string, { source?: string; path?: string }> } =>
    JSON.parse(sh("cat /root/.paseo/config.json"));
  const pluginLogLines = (): number => paseo("plugin", "logs", PLUGIN_ID).split("\n").length;

  // Relative path -> SHA-256 of every file the app ships for the probe plugin.
  function hostDigests(): Record<string, string> {
    const out: Record<string, string> = {};
    const walk = (dir: string) => {
      for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
        if (entry.name === "node_modules" || entry.name.startsWith(".")) continue;
        const full = path.join(dir, entry.name);
        if (entry.isDirectory()) walk(full);
        else out[path.relative(HOST_PLUGIN_DIR, full).split(path.sep).join("/")] =
          crypto.createHash("sha256").update(fs.readFileSync(full)).digest("hex");
      }
    };
    walk(HOST_PLUGIN_DIR);
    return out;
  }

  function containerDigests(): Record<string, string> {
    const listing = sh(`cd ${PLUGIN_DIR}/${PLUGIN_ID} && find . -type f | sort | xargs sha256sum`);
    const out: Record<string, string> = {};
    for (const line of listing.split("\n").filter(Boolean)) {
      const [digest, file] = line.trim().split(/\s+/);
      out[file.replace(/^\.\//, "")] = digest;
    }
    return out;
  }

  const bundleHash = (): string => sh(`cat ${PLUGIN_DIR}/.bundle-hash`).trim();
  const loadedHash = (): string => sh("cat /root/.paseo/mio-plugins-loaded").trim();

  async function waitForPluginRunning(): Promise<void> {
    await browser.waitUntil(
      () => paseo("plugin", "ls").split("\n").some((l) => l.includes(PLUGIN_ID) && l.includes("running")),
      {
        timeout: AGENT_SETTLE,
        interval: 2000,
        timeoutMsg: `paseo plugin ls never listed ${PLUGIN_ID} as running:\n${paseo("plugin", "ls")}`,
      },
    );
  }

  // Closing the pane first makes the open run prepare_agent_web() again instead of only switching.
  async function reopen(): Promise<void> {
    await closePane(projectName, "agentweb");
    await browser.waitUntil(
      async () => paneFor(await workspaceState(), projectName, "agentweb") === undefined,
      { timeout: 30000, interval: 1000, timeoutMsg: "close_pane left the agentweb pane in place." },
    );
    await openAgentWeb(projectName, workspaceDir);
    assert(paneFor(await workspaceState(), projectName, "agentweb"), "open_agent_web created no agentweb pane.");
  }

  it("creates and starts a sandbox", async () => {
    await ProjectsPage.open();
    await ProjectsPage.createProject(projectName, { timeout: 600000 });
    await row.start();
    await row.waitForRunning();
    await row.openWorkspace();
  });

  it("the first Agents open delivers the plugins and registers them with the daemon", async () => {
    await openAgentWeb(projectName, workspaceDir);
    await browser.waitUntil(
      async () => paneFor(await workspaceState(), projectName, "agentweb") !== undefined,
      { timeout: 840000, interval: 3000, timeoutMsg: "no agentweb pane appeared after open_agent_web." },
    );

    const hash = bundleHash();
    assert.match(hash, /^[0-9a-f]{64}$/, `.bundle-hash is not a SHA-256 digest: "${hash}"`);
    assert.strictEqual(loadedHash(), hash, "mio-plugins-loaded does not match the delivered bundle hash.");

    // Exact map equality also rules out a stray node_modules or dotfile in the delivered copy.
    assert.deepStrictEqual(
      containerDigests(),
      hostDigests(),
      `The delivered ${PLUGIN_ID} files differ from the repository's paseo-plugins/${PLUGIN_ID}/.`,
    );

    const config = readConfig();
    assert.strictEqual(config.pluginsEnabled, true, "config.json does not enable plugins.");
    assert.deepStrictEqual(
      config.plugins?.[PLUGIN_ID],
      { source: "directory", path: `${PLUGIN_DIR}/${PLUGIN_ID}` },
      `config.json has no directory entry for ${PLUGIN_ID}: ${JSON.stringify(config.plugins)}`,
    );

    await waitForPluginRunning();

    // App-owned plugins live outside the synced workspace.
    assert.strictEqual(sh(`find /root/work -name ${PLUGIN_ID} | wc -l`).trim(), "0");
  });

  it("re-opening with unchanged plugins neither restarts nor reloads", async () => {
    const pid = daemonPid();
    const lines = pluginLogLines();
    await reopen();
    assert.strictEqual(daemonPid(), pid, "The daemon restarted although nothing changed.");
    assert.strictEqual(pluginLogLines(), lines, "The plugin was reloaded although its contents did not change.");
  });

  it("changed plugin contents reload in place without a daemon restart", async () => {
    const pid = daemonPid();
    const hash = bundleHash();
    const lines = pluginLogLines();
    // The state after the app ships new contents: the loaded marker lags the delivered bundle.
    sh("printf stale > /root/.paseo/mio-plugins-loaded");

    await reopen();
    assert.strictEqual(daemonPid(), pid, "A contents-only change restarted the daemon.");
    assert.strictEqual(loadedHash(), hash, "The loaded marker was not updated after the reload.");
    await browser.waitUntil(() => pluginLogLines() > lines, {
      timeout: AGENT_SETTLE,
      interval: 2000,
      timeoutMsg: "paseo plugin logs shows no new lines, so no reload ran.",
    });
    await waitForPluginRunning();
  });

  it("a stale delivered copy is redelivered", async () => {
    const pid = daemonPid();
    const hash = bundleHash();
    sh(`echo junk >> ${PLUGIN_DIR}/${PLUGIN_ID}/index.client.tsx && rm ${PLUGIN_DIR}/.bundle-hash`);

    await reopen();
    assert.strictEqual(bundleHash(), hash, ".bundle-hash was not restored by the redelivery.");
    assert.deepStrictEqual(containerDigests(), hostDigests(), "The corrupted file was not replaced.");
    assert.strictEqual(daemonPid(), pid, "Redelivering an unchanged plugin set restarted the daemon.");
  });

  it("a changed plugin set restarts the daemon and prunes entries without a folder", async () => {
    const pid = daemonPid();
    paseo("daemon", "stop");
    // Edited while the daemon is down, because the daemon rewrites config.json itself.
    sh(
      "cd /root/work && mise exec -- node -e '" +
        "const f=\"/root/.paseo/config.json\",fs=require(\"fs\"),c=JSON.parse(fs.readFileSync(f,\"utf8\"));" +
        "c.plugins[\"mio-ghost\"]={source:\"directory\",path:\"/nonexistent/mio-ghost\"};" +
        "c.plugins[\"user-plugin\"]={source:\"directory\",path:\"/nonexistent/user-plugin\"};" +
        "fs.writeFileSync(f,JSON.stringify(c))' && rm /root/.paseo/mio-launch",
    );

    await reopen();
    assert.notStrictEqual(daemonPid(), pid, "The daemon was not restarted.");
    const config = readConfig();
    assert.strictEqual(config.pluginsEnabled, true);
    assert(config.plugins?.[PLUGIN_ID], `${PLUGIN_ID} lost its config entry: ${JSON.stringify(config.plugins)}`);
    assert(!config.plugins?.["mio-ghost"], "The mio-ghost entry without a folder was not pruned.");
    assert(config.plugins?.["user-plugin"], "A non-mio plugin entry was not preserved.");
    await waitForPluginRunning();
  });

  it("destroy removes the container, and the plugins with it", async () => {
    await row.destroy();
    assert(
      !podmanOk(LOCAL_CONNECTION, ["inspect", projectName]),
      `Container "${projectName}" still exists after destroy.`,
    );
  });
});
