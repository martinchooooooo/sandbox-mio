import assert from "node:assert";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import ProjectsPage from "../../pageobjects/projects.page.js";
import { invokeCmd, mockCalls, useMockControl } from "../../../lib/mock.js";

// LIGHT spec for docs/features/microsandbox-network-policy (Phase D exit). The `msb create` argv is
// read back from the mock call journal (`mockCalls`), which records variable names but never
// values, so the spec can prove a secret travels only in the child environment.
const WORKSPACE_ROOT = process.env.SANDBOX_WORKSPACE_ROOT ?? "";

describe("Microsandbox network policy — create flow (mock backend)", function () {
  this.timeout(60000);
  useMockControl();

  const allowName = `e2e-netpol-${Date.now()}`;
  const secretName = `e2e-netpol-sec-${Date.now()}`;
  const viewerName = `e2e-netpol-view-${Date.now()}`;
  const reapplyName = `e2e-netpol-re-${Date.now()}`;
  let importDir = "";

  after(async () => {
    for (const name of [allowName, secretName, viewerName, reapplyName]) {
      await invokeCmd("destroy_sandbox", { name, force: true }).catch(() => undefined);
    }
    if (importDir) fs.rmSync(importDir, { recursive: true, force: true });
  });

  it("seeds an allowlist policy and renders deny-by-default flags", async () => {
    await ProjectsPage.goHome();
    await ProjectsPage.open();
    await ProjectsPage.ensurePodmanReady();

    await ProjectsPage.chooseProvider("microsandbox");
    await ProjectsPage.chooseEgress("allowlist");
    await ProjectsPage.createProject(allowName);

    const policyFile = path.join(WORKSPACE_ROOT, allowName, ".mio", "net-policy.json");
    const policy = JSON.parse(fs.readFileSync(policyFile, "utf8"));
    assert.equal(policy.egress, "allowlist");
    const hosts = policy.rules.map((r: { host?: string }) => r.host);
    assert(hosts.includes("registry.npmjs.org"), "the static starter base list was not seeded");
    assert(hosts.includes("api.anthropic.com"), "the catalogue agents' apiHosts were not seeded");

    const create = (await mockCalls("msb.create")).find((c) => c.argv.includes(allowName));
    assert(create, "no msb create was journalled for the sandbox");
    const argv = create.argv.join(" ");
    assert(argv.includes("--net-default-egress deny"), argv);
    assert(argv.includes("--tls-intercept"), argv);
    assert(argv.includes("--net-rule allow@registry.npmjs.org:tcp:443"), argv);
    assert(!argv.includes("allow@dns"), "allow@dns opens the resolver to every name");
  });

  it("passes a bound secret by name only", async () => {
    const value = `e2e-secret-value-${Date.now()}`;
    await invokeCmd("set_proxy_secret", { secretId: "e2e-gh", value });

    importDir = fs.mkdtempSync(path.join(os.tmpdir(), "mio-netpol-"));
    fs.mkdirSync(path.join(importDir, ".mio"));
    fs.writeFileSync(
      path.join(importDir, ".mio", "net-policy.json"),
      JSON.stringify({
        egress: "allowlist",
        rules: [{ action: "allow", host: "api.github.com", protocol: "tcp", ports: [443] }],
        secrets: [{ secretId: "e2e-gh", envVar: "GH_TOKEN", hosts: ["api.github.com"] }],
      }),
    );

    await invokeCmd("create_sandbox", {
      name: secretName,
      existingPath: importDir,
      provider: "microsandbox",
      egress: "allowlist",
      starterHosts: [],
    });

    const create = (await mockCalls("msb.create")).find((c) => c.argv.includes(secretName));
    assert(create, "no msb create was journalled for the sandbox");
    assert.deepEqual(create.env_names, ["GH_TOKEN"]);
    const argv = create.argv.join(" ");
    assert(argv.includes("--secret GH_TOKEN:headers=[authorization]@api.github.com"), argv);
    assert(!argv.includes(value), "the secret value reached argv");
    // An imported workspace's own rules win: no starter hosts were merged into a non-empty list.
    assert(!argv.includes("registry.npmjs.org"), argv);

    const starts = (await mockCalls("msb.start")).filter((c) => c.argv.includes(secretName));
    assert(starts.every((c) => c.env_names.includes("GH_TOKEN")), "a start ran without the secret");
  });

  it("chains a viewer-enabled sandbox through a sidecar VM (Phase F exit)", async () => {
    await invokeCmd("create_sandbox", { name: viewerName, provider: "microsandbox", proxy: true });

    const listed = await invokeCmd<{ name: string }[]>("list_sandboxes");
    const names = listed.map((s) => s.name).filter((n) => n.startsWith(viewerName));
    assert.deepEqual(names, [viewerName], "the -proxy sidecar leaked into list_sandboxes");

    const creates = await mockCalls("msb.create");
    const sidecar = creates.find((c) => c.argv.includes(`${viewerName}-proxy`));
    assert(sidecar, "no sidecar msb create was journalled");
    const sidecarArgv = sidecar.argv.join(" ");
    assert(sidecarArgv.includes("-p 127.0.0.1:"), sidecarArgv);
    assert(sidecarArgv.includes("--init /.msb/scripts/start"), sidecarArgv);

    const agent = creates.find((c) => c.argv.includes(viewerName));
    assert(agent, "no agent msb create was journalled");
    assert(agent.argv.join(" ").includes("--proxy socks5://127.0.0.1:"), agent.argv.join(" "));
    assert(agent.env_names.includes("MIO_SOCKS_PASSWORD"), "the SOCKS password is not in the start env");

    const status = await invokeCmd<{ provider: string; running: boolean }>("proxy_status", { name: viewerName });
    assert.equal(status.provider, "microsandbox");
    assert(status.running, "the sidecar is not reported running");
  });

  it("re-applies an edited policy by recreating around the volume (Phase G exit)", async () => {
    await invokeCmd("create_sandbox", { name: reapplyName, provider: "microsandbox", egress: "open", starterHosts: [] });
    const workspacePath = path.join(WORKSPACE_ROOT, reapplyName).replace(/\\/g, "/");
    const policy = await invokeCmd<{ rules: unknown[] }>("get_net_policy", { workspacePath });
    policy.rules = [{ action: "deny", host: "example.com", protocol: "tcp", ports: [443] }];
    await invokeCmd("set_net_policy", { workspacePath, policy, provider: "microsandbox" });
    assert(await invokeCmd<boolean>("net_policy_drift", { name: reapplyName, workspacePath }), "no drift reported");

    await invokeCmd("reapply_sandbox_network", { name: reapplyName, viewer: false });

    assert((await mockCalls("msb.rm")).some((c) => c.argv.includes(reapplyName)), "the old VM was not removed");
    assert(
      !(await mockCalls("msb.volume-rm")).some((c) => c.argv.includes(`${reapplyName}-ws`)),
      "the workspace volume was removed",
    );
    const creates = (await mockCalls("msb.create")).filter((c) => c.argv.includes(reapplyName));
    const last = creates[creates.length - 1].argv.join(" ");
    assert(last.includes("--net-rule deny@example.com:tcp:443"), last);
    assert(last.includes("--net-rule allow@public"), last);
    assert(!(await invokeCmd<boolean>("net_policy_drift", { name: reapplyName, workspacePath })), "drift stayed");
  });
});
