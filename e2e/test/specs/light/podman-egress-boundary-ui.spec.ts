import assert from "node:assert";
import fs from "node:fs";
import path from "node:path";

import ProjectsPage from "../../pageobjects/projects.page.js";
import workspaceDashboard from "../../pageobjects/workspace-dashboard.page.js";
import { captureDiagOnFailure } from "../../../lib/diag.js";
import { configureMock, invokeCmd, mockCalls, useMockControl } from "../../../lib/mock.js";
import { APP_DATA_ROOT, WORKSPACE_ROOT, WORKTREE_ROOT } from "../../../lib/run-identity.js";
import { COMMAND } from "../../../lib/timeouts.js";

// LIGHT companion to standard/podman-egress-boundary.spec.ts (docs/features/podman-egress-boundary/03_e2e.md):
// the editor's gating and failure paths against the scripted Podman runner. The standard spec is the proof.

const FIXTURE = path.join(WORKTREE_ROOT, "docs/features/podman-egress-boundary/spike/fixtures/podman-logs-timestamps.txt");

describe("Podman egress boundary — editor gating and failure paths (mock backend)", function () {
  this.timeout(60000);
  captureDiagOnFailure();
  useMockControl();

  const stamp = Date.now();
  const name = `e2e-pebui-${stamp}`;
  const legacyName = `e2e-pebui-legacy-${stamp}`;
  const workspacePath = path.join(WORKSPACE_ROOT, name);
  const policyFile = path.join(workspacePath, ".mio", "net-policy.json");
  const stateDir = path.join(APP_DATA_ROOT, "proxy", name);

  async function openNetwork(sandbox: string): Promise<void> {
    await ProjectsPage.row(sandbox).openWorkspace();
    await workspaceDashboard.waitForRendered(sandbox);
    // Re-entering the section remounts the editor, which re-reads the policy file and the drift.
    await workspaceDashboard.openSection("overview");
    await workspaceDashboard.openSection("network");
  }

  after(async () => {
    for (const n of [name, legacyName]) await invokeCmd("destroy_sandbox", { name: n, force: true }).catch(() => undefined);
  });

  it("shows the egress picker for Podman only once the proxy switch is on", async () => {
    await ProjectsPage.goHome();
    await ProjectsPage.open();
    await ProjectsPage.ensurePodmanReady();
    await ProjectsPage.chooseProvider("podman");
    assert(!(await ProjectsPage.egressOption("allowlist").isExisting()), "egress picker shown with the proxy off");
    await ProjectsPage.enableProxy();
    await ProjectsPage.egressOption("allowlist").waitForDisplayed({ timeout: COMMAND });
    assert(await ProjectsPage.podmanProxyHint.isDisplayed(), "the Podman proxy hint is missing");
    await ProjectsPage.chooseEgress("allowlist");
    await ProjectsPage.createProject(name);

    // Mock assumptions: the tracked `podman ps` reports an empty Image, which `is_sidecar` must still
    // classify as a sidecar, and unscripted `exec`/`network-*` calls succeed with empty output.
    const listed = (await invokeCmd<{ name: string }[]>("list_sandboxes")).map((s) => s.name);
    assert(listed.includes(name) && !listed.includes(`${name}-proxy`), `sidecar leaked into the list: ${listed}`);
    const status = await invokeCmd<{ exists: boolean; engine: string }>("proxy_status", { name });
    assert(status.exists && status.engine === "iron-proxy", JSON.stringify(status));
    assert((await mockCalls("podman.network-create")).length > 0, "no internal network was created");
    assert((await mockCalls("podman.exec")).length > 0, "neither inject_ca nor the reload ran");
  });

  it("renders the sidecar dialect of the editor", async () => {
    await openNetwork(name);
    await $("[data-net-policy-sidecar-hint]").waitForExist({ timeout: COMMAND });
    assert(!(await $("[data-net-policy-reapply]").isExisting()), "the recreate block is not for the sidecar dialect");
    assert(!(await $("[data-net-policy-viewer]").isExisting()), "the viewer switch is not for the sidecar dialect");
    assert(!(await $("button=block-and-log").isExisting()), "violation actions are not for the sidecar dialect");
    assert(!(await $("[data-proxy-legacy]").isExisting()), "a new sidecar must not show the legacy notice");
  });

  it("shows drift and an unstamped binding after an on-disk edit, and lists its missing value on apply", async () => {
    const policy = JSON.parse(fs.readFileSync(policyFile, "utf8"));
    policy.rules.push({ action: "allow", host: "api.example.com" });
    policy.secrets.push({ secretId: `pebui-${stamp}`, envVar: "PEBUI_KEY", hosts: ["api.example.com"], headers: ["authorization"] });
    fs.writeFileSync(policyFile, JSON.stringify(policy, null, 2));

    await openNetwork(name);
    await workspaceDashboard.netPolicyDrift.waitForExist({ timeout: COMMAND });
    const unstamped = $("[data-net-policy-unstamped='PEBUI_KEY']");
    await unstamped.waitForExist({ timeout: COMMAND });
    assert.match(await unstamped.getText(), /export PEBUI_KEY=mio-placeholder-pebui_key/);

    // Drift alone enables Save: the file is applied unchanged.
    await workspaceDashboard.saveNetPolicy();
    assert.match(await workspaceDashboard.applyReportText(), /\$PEBUI_KEY/);
    // The report renders before the editor's refresh clears the drift flag.
    await workspaceDashboard.netPolicyDrift.waitForExist({ reverse: true, timeout: COMMAND, timeoutMsg: "drift must clear after the apply" });
  });

  it("surfaces a rejected reload and keeps the running config on disk", async () => {
    const configFile = path.join(stateDir, "config", "config.yaml");
    const before = fs.readFileSync(configFile);
    await configureMock("podman.exec", { kind: "fail-once", exit_code: 1, stderr: "wget: server returned error: HTTP/1.1 400" });

    await openNetwork(name);
    await workspaceDashboard.addAllowRule("example.org");
    await workspaceDashboard.netPolicySave.click();
    const error = $("[data-net-policy-error]");
    await error.waitForExist({ timeout: COMMAND });
    assert.match(await error.getText(), /reload failed/);
    assert(fs.readFileSync(configFile).equals(before), "config.yaml must match the pipeline that still runs");
  });

  it("translates the sidecar's audit lines into rejected and would-block rows", async () => {
    await configureMock("podman.logs", { kind: "return-value", stdout: fs.readFileSync(FIXTURE, "utf8") });

    await ProjectsPage.row(name).openWorkspace();
    await workspaceDashboard.waitForRendered(name);
    await workspaceDashboard.openSection("traffic");
    await workspaceDashboard.waitForFlowHost("www.wikipedia.org", { rejected: true });
    assert(await $("[data-flow-would-block]").isExisting(), "no would-block row rendered");
  });

  it("shows the legacy notice and no editor for a sidecar without an engine", async () => {
    await invokeCmd("create_sandbox", { name: legacyName, provider: "podman", proxy: true });
    const metaFile = path.join(APP_DATA_ROOT, "proxy", legacyName, "meta.json");
    const meta = JSON.parse(fs.readFileSync(metaFile, "utf8"));
    delete meta.engine;
    fs.writeFileSync(metaFile, JSON.stringify(meta));

    await openNetwork(legacyName);
    await $("[data-proxy-legacy]").waitForExist({ timeout: COMMAND });
    assert(!(await $("[data-net-policy-editor]").isExisting()), "a legacy sidecar must not get the editor");
  });
});
