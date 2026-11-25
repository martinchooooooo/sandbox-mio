import assert from "node:assert";
import fs from "node:fs";
import path from "node:path";

import ProjectsPage from "../../pageobjects/projects.page.js";
import workspaceDashboard from "../../pageobjects/workspace-dashboard.page.js";
import { captureDiagOnFailure } from "../../../lib/diag.js";
import { invokeCmd, useMockControl } from "../../../lib/mock.js";
import { APP_DATA_ROOT, WORKSPACE_ROOT } from "../../../lib/run-identity.js";
import { COMMAND } from "../../../lib/timeouts.js";

// The workspace policy file is a proposal, never an apply input (docs/features/net-policy-review/02_dev.md).
// Light only: it must not run deploy_sandbox past plan_deploy, so deploy is checked through the secret count.

interface Binding { secretId: string; envVar: string; hosts: string[]; headers: string[]; query: boolean; body: boolean }
interface Policy { egress: string; rules: { action: string; host?: string }[]; secrets: Binding[] }
interface State { accepted: Policy; revision: number; proposal: { changes: { widens: boolean }[] } | null }

describe("Net policy review — the workspace file is a proposal (mock backend)", function () {
  this.timeout(120000);
  captureDiagOnFailure();
  useMockControl();

  const stamp = Date.now();
  const name = `e2e-npr-${stamp}`;
  const other = `e2e-npr2-${stamp}`;
  const policyFile = path.join(WORKSPACE_ROOT, name, ".mio", "net-policy.json");
  const stateDir = path.join(APP_DATA_ROOT, "proxy", name);
  const appliedFile = path.join(APP_DATA_ROOT, "net-policy", name, "applied.json");
  const EVIL = "evil.example.net";

  const readFile = (): Policy => JSON.parse(fs.readFileSync(policyFile, "utf8"));
  const state = () => invokeCmd<State>("net_policy_state", { name });
  const configText = () => fs.readFileSync(path.join(stateDir, "config", "config.yaml"), "utf8");

  /** Catches inside the webview, which keeps the AppError a rejection would otherwise stringify away. */
  async function applyExpectingError(args: Record<string, unknown>): Promise<{ message: string } | null> {
    return browser.execute(async (a) => {
      try {
        // @ts-expect-error __TAURI_INTERNALS__ is injected into the webview at runtime.
        await window.__TAURI_INTERNALS__.invoke("apply_sandbox_policy", a);
        return null;
      } catch (e) {
        return e as { message: string };
      }
    }, args);
  }

  async function openNetwork(): Promise<void> {
    await ProjectsPage.row(name).openWorkspace();
    await workspaceDashboard.waitForRendered(name);
    // Re-entering the section remounts the editor, which re-reads the accepted policy and proposal.
    await workspaceDashboard.openSection("overview");
    await workspaceDashboard.openSection("network");
  }

  /** The file as the sandbox could rewrite it: the accepted binding widened to `EVIL`, plus an allow rule. */
  function writeWidening(): void {
    const file = readFile();
    file.secrets[0].hosts.push(EVIL);
    file.rules.push({ action: "allow", host: EVIL });
    fs.writeFileSync(policyFile, JSON.stringify(file, null, 2));
  }

  after(async () => {
    for (const n of [name, other]) await invokeCmd("destroy_sandbox", { name: n, force: true }).catch(() => undefined);
  });

  it("accepts a secret through the editor and mirrors it to the workspace file", async () => {
    await ProjectsPage.goHome();
    await ProjectsPage.open();
    await ProjectsPage.ensurePodmanReady();
    await ProjectsPage.createProject(name);
    await openNetwork();
    await workspaceDashboard.addSecret("NPR_KEY", "api.example.com", `value-${stamp}`);
    assert.deepEqual(readFile().secrets.map((b) => b.envVar), ["NPR_KEY"]);
    assert.equal((await state()).proposal, null, "the mirror must leave no proposal behind");
  });

  it("shows a widened workspace file as a proposal while the accepted binding stays narrow", async () => {
    writeWidening();
    await openNetwork();
    await workspaceDashboard.netPolicyProposal.waitForExist({ timeout: COMMAND });
    const row = workspaceDashboard.secretRow("NPR_KEY");
    await row.waitForExist({ timeout: COMMAND });
    const text = await row.getText();
    assert(text.includes("api.example.com") && !text.includes(EVIL), `accepted binding row: ${text}`);
    assert(!(await $("[data-secret-save]").isEnabled()), "secret controls must be disabled while a proposal is pending");
  });

  it("never lets an apply from the editor's store path enforce the proposal", async () => {
    const s = await state();
    assert(s.proposal?.changes.some((c) => c.widens), "the widening must be flagged");
    const accepted = s.accepted;
    const second: Binding = { secretId: `${name}-npr_two`, envVar: "NPR_TWO", hosts: ["two.example.com"], headers: ["authorization"], query: false, body: false };
    const policy = { ...accepted, secrets: [...accepted.secrets, second], rules: accepted.egress === "allowlist" ? [...accepted.rules, { action: "allow", host: "two.example.com" }] : accepted.rules };
    await invokeCmd("apply_sandbox_policy", { name, policy, baseRevision: s.revision, storeSecret: { secretId: second.secretId, value: `two-${stamp}` } });

    assert(!configText().includes(EVIL), "config.yaml enforces the unreviewed host");
    assert(!fs.readFileSync(appliedFile, "utf8").includes(EVIL), "applied.json holds the unreviewed host");
    assert(configText().includes("two.example.com"), "the editor's own change must apply");
    assert(readFile().secrets[0].hosts.includes(EVIL), "the pending proposal must survive an apply that does not take it in");

    const stale = await applyExpectingError({ name, policy, baseRevision: s.revision });
    assert(stale && /reload/.test(stale.message), `an apply from an out-of-date revision must be refused: ${JSON.stringify(stale)}`);
  });

  it("discards a proposal by mirroring the accepted policy", async () => {
    await openNetwork();
    await workspaceDashboard.netPolicyProposal.waitForExist({ timeout: COMMAND });
    await workspaceDashboard.reviewProposal();
    const flags = await workspaceDashboard.proposalWidenFlags();
    assert(flags.length > 0 && flags[0], `the widening change must be listed first: ${flags}`);
    assert.match(await $("[data-proposal-risk]").getText(), /Requests to evil\.example\.net will receive the real value of \$NPR_KEY/);
    await $("[data-proposal-cancel]").click();
    await $("[data-net-policy-proposal-dialog]").waitForExist({ reverse: true, timeout: COMMAND });
    await workspaceDashboard.discardProposal();
    assert.deepEqual(readFile(), (await state()).accepted, "the file must equal the accepted policy after Discard");
  });

  it("applies exactly the reviewed proposal", async () => {
    writeWidening();
    await openNetwork();
    await workspaceDashboard.netPolicyProposal.waitForExist({ timeout: COMMAND });
    await workspaceDashboard.reviewProposal();
    await workspaceDashboard.applyProposal();
    assert(configText().includes(EVIL), "the reviewed host must be enforced after Apply");
    assert(fs.readFileSync(appliedFile, "utf8").includes(EVIL), "applied.json must record the reviewed host");
    assert.equal((await state()).proposal, null);
  });

  it("names the secret a proposal really exposes, not the variable it is bound under", async () => {
    const s = await state();
    const npr = s.accepted.secrets.find((b) => b.envVar === "NPR_KEY")!;
    const file = readFile();
    file.secrets.push({ ...npr, envVar: "EXFIL", hosts: [EVIL] });
    fs.writeFileSync(policyFile, JSON.stringify(file, null, 2));
    await openNetwork();
    await workspaceDashboard.netPolicyProposal.waitForExist({ timeout: COMMAND });
    await workspaceDashboard.reviewProposal();
    const risk = await $("[data-proposal-risk]").getText();
    assert.match(risk, /Requests to evil\.example\.net will receive the real value stored for \$NPR_KEY/);
    assert(!risk.includes("$EXFIL"), `the sentence must not name the proposal's variable: ${risk}`);
    await $("[data-proposal-cancel]").click();
    await $("[data-net-policy-proposal-dialog]").waitForExist({ reverse: true, timeout: COMMAND });
    await workspaceDashboard.discardProposal();
  });

  it("resolves a secret id only within its own sandbox and counts only accepted bindings", async () => {
    await invokeCmd("create_sandbox", { name: other });
    const own = (await invokeCmd<State>("net_policy_state", { name: other }));
    const otherId = `${other}-key`;
    const binding: Binding = { secretId: otherId, envVar: "OTHER_KEY", hosts: ["api.example.com"], headers: ["authorization"], query: false, body: false };
    const withRule = (rules: Policy["rules"]) => (own.accepted.egress === "allowlist" ? [...rules, { action: "allow", host: "api.example.com" }] : rules);
    await invokeCmd("apply_sandbox_policy", {
      name: other,
      policy: { ...own.accepted, secrets: [binding], rules: withRule(own.accepted.rules) },
      baseRevision: own.revision,
      storeSecret: { secretId: otherId, value: `other-${stamp}` },
    });

    const s = await state();
    const stolen: Binding = { ...binding, envVar: "STOLEN" };
    const report = await invokeCmd<{ missingSecrets: string[] }>("apply_sandbox_policy", {
      name,
      policy: { ...s.accepted, secrets: [...s.accepted.secrets, stolen], rules: withRule(s.accepted.rules) },
      baseRevision: s.revision,
    });
    assert(report.missingSecrets.includes("STOLEN"), "another sandbox's secret id must resolve to nothing");
    assert(!fs.existsSync(path.join(stateDir, "secrets", "STOLEN")), "no secret file may exist for a foreign id");

    // Counts the accepted bindings that have a value, not what a rewritten file would add.
    const count = await invokeCmd<number>("deploy_secret_count", { name });
    writeWidening();
    assert.equal(count, 2, "NPR_KEY and NPR_TWO have stored values; STOLEN has none");
    assert.equal(await invokeCmd<number>("deploy_secret_count", { name }), count, "a widened file must not change the count");
  });
});
