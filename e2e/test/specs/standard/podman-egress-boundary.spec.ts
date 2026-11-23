import assert from "node:assert";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

import ProjectsPage from "../../pageobjects/projects.page.js";
import workspaceDashboard from "../../pageobjects/workspace-dashboard.page.js";
import { captureDiagOnFailure } from "../../../lib/diag.js";
import { invokeCmd } from "../../../lib/mock.js";
import { LOCAL_CONNECTION, podman, podmanOk } from "../../../lib/podman-connection.js";
import { APP_DATA_ROOT, WORKSPACE_ROOT } from "../../../lib/run-identity.js";
import { COMMAND, PACKAGE_INSTALL, SUBPROCESS } from "../../../lib/timeouts.js";

// STANDARD spec for docs/features/podman-egress-boundary (03_e2e.md): a proxied Podman sandbox sits on
// an internal network behind a dual-homed iron-proxy sidecar. The light tier cannot prove that raw
// sockets are unreachable, that a policy change reloads instead of restarting, or that the real secret
// reaches the upstream while the guest holds only the placeholder.
//
// The policy is seeded into the workspace directory before create: placeholders are stamped with
// `--env` at `podman create` only (02_dev.md, Resolved Decision 7), so a binding added later would
// leave the variable unset in the guest.

const ENV_VAR = "E2E_PEB_KEY";
const PLACEHOLDER = "mio-placeholder-e2e_peb_key";
const ECHO_HOST = "postman-echo.com";

interface GuestCurl {
  status: number | null;
  code: string;
  body: string;
  stderr: string;
}

/** `podman exec` inherits the container's create-time env, so it carries proxy_env, the CA and placeholders. */
function guestSh(name: string, script: string): { status: number | null; stdout: string; stderr: string } {
  const r = spawnSync("podman", ["--connection", LOCAL_CONNECTION, "exec", name, "sh", "-c", script], {
    encoding: "utf8",
    timeout: SUBPROCESS,
  });
  return { status: r.status, stdout: r.stdout.trim(), stderr: r.stderr.trim() };
}

/** curl inside the sandbox. The status code is appended after the body on its own line (`000` = no response). */
function guestCurl(name: string, url: string, { noProxy = false, headers = [] as string[] } = {}): GuestCurl {
  const flags = [
    "-sS",
    "--max-time 15",
    noProxy ? "--noproxy '*'" : "",
    ...headers.map((h) => `-H "${h}"`),
    `-w '\\n%{http_code}'`,
  ].join(" ");
  const r = guestSh(name, `curl ${flags} ${url}`);
  const lines = r.stdout.split("\n");
  const code = lines.pop() ?? "";
  return { status: r.status, code, body: lines.join("\n"), stderr: r.stderr };
}

function readFileIfExists(file: string): string {
  return fs.existsSync(file) ? fs.readFileSync(file, "utf8") : "";
}

/** Every file under `dir`, recursively. */
function walk(dir: string): string[] {
  if (!fs.existsSync(dir)) return [];
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = path.join(dir, e.name);
    return e.isDirectory() ? walk(p) : [p];
  });
}

describe("Podman egress boundary — bounded sandbox on a real machine", function () {
  captureDiagOnFailure();
  this.timeout(PACKAGE_INSTALL + 5 * 60_000);

  const stamp = Date.now();
  const name = `e2e-peb-${stamp}`;
  const sidecar = `${name}-proxy`;
  const network = `mio-proxy-${name}`;
  const secretId = `e2e-peb-secret-${stamp}`;
  // Must not be a substring of the sandbox or sidecar name, or the absence search matches the name.
  const secretValue = `secretvalue-${stamp}-x`;
  const workspaceDir = path.join(WORKSPACE_ROOT, name);
  const stateDir = path.join(APP_DATA_ROOT, "proxy", name);
  const policyFile = path.join(workspaceDir, ".mio", "net-policy.json");
  const runStart = Date.now();

  after(async () => {
    // A failed run must not leave a sidecar or an internal network on the shared mio-tests machine.
    await invokeCmd("destroy_sandbox", { name, force: true }).catch(() => undefined);
  });

  it("creates a bounded sandbox from a seeded policy", async () => {
    fs.mkdirSync(path.dirname(policyFile), { recursive: true });
    fs.writeFileSync(
      policyFile,
      JSON.stringify(
        {
          version: 1,
          egress: "allowlist",
          rules: [{ action: "allow", host: ECHO_HOST }],
          secrets: [{ secretId, envVar: ENV_VAR, hosts: [ECHO_HOST], headers: ["authorization"] }],
        },
        null,
        2,
      ),
    );

    await ProjectsPage.open();
    await ProjectsPage.ensurePodmanReady();
    await ProjectsPage.openAdvanced();
    await ProjectsPage.egressOption("allowlist").waitForDisplayed({ timeout: COMMAND });
    // The dialog always sends its mode and the default `open` would overwrite the seeded allowlist.
    await ProjectsPage.chooseEgress("allowlist");

    // The first proxied create of a fresh machine pulls the iron-proxy image.
    const row = await ProjectsPage.createProject(name, { timeout: PACKAGE_INSTALL });
    // inject_ca starts the sandbox during create, so there is no Start to click.
    await row.waitForRunning();
  });

  it("is attached only to the internal network behind the iron-proxy sidecar", () => {
    assert.equal(podman(LOCAL_CONNECTION, ["network", "inspect", network, "--format", "{{.Internal}}"]), "true");

    const sandboxNets = Object.keys(
      JSON.parse(podman(LOCAL_CONNECTION, ["inspect", name, "--format", "{{json .NetworkSettings.Networks}}"])),
    );
    assert.deepEqual(sandboxNets, [network]);

    assert.equal(podman(LOCAL_CONNECTION, ["inspect", sidecar, "--format", "{{.State.Running}}"]), "true");
    assert.match(podman(LOCAL_CONNECTION, ["inspect", sidecar, "--format", "{{.ImageName}}"]), /iron-proxy/);
    const sidecarNets = Object.keys(
      JSON.parse(podman(LOCAL_CONNECTION, ["inspect", sidecar, "--format", "{{json .NetworkSettings.Networks}}"])),
    ).sort();
    assert.deepEqual(sidecarNets, [network, "podman"].sort());

    const meta = JSON.parse(fs.readFileSync(path.join(stateDir, "meta.json"), "utf8"));
    assert.equal(meta.engine, "iron-proxy");

    const applied = JSON.parse(
      fs.readFileSync(path.join(APP_DATA_ROOT, "net-policy", name, "applied.json"), "utf8"),
    );
    assert(applied.guestEnv.includes(ENV_VAR), "applied.json must record the create-time placeholder variable");

    assert.equal(guestSh(name, `printenv ${ENV_VAR}`).stdout, PLACEHOLDER);
  });

  it("has no direct route out and refuses an unlisted host", () => {
    const raw = guestCurl(name, "https://1.1.1.1/", { noProxy: true });
    assert.notEqual(raw.status, 0, "a raw-IP connection must fail: the sandbox has no route out");
    assert.equal(raw.code, "000", `a raw-IP connection must get no HTTP response (got ${raw.code})`);

    const denied = guestCurl(name, "https://example.com/");
    assert.notEqual(denied.status, 0, "example.com is not allowlisted, so the proxy must refuse the CONNECT");
  });

  it("passes the placeholder through while the binding has no stored value", () => {
    const r = guestCurl(name, `https://${ECHO_HOST}/headers`, {
      headers: [`Authorization: Bearer $${ENV_VAR}`],
    });
    assert.equal(r.code, "200", `${r.status} ${r.stderr}`);
    assert(r.body.includes(PLACEHOLDER), `the upstream must see the placeholder: ${r.body}`);
  });

  it("swaps the placeholder for the real value once the secret is stored", async () => {
    await ProjectsPage.row(name).openWorkspace();
    await workspaceDashboard.waitForRendered(name);
    await workspaceDashboard.openSection("network");
    await $("[data-proxy-non-http-blocked]").waitForExist({
      timeout: COMMAND,
      timeoutMsg: "the non-HTTP-blocked line never appeared (ProxyPanel needs a running iron-proxy sidecar status)",
    });

    await workspaceDashboard.editSecret(ENV_VAR, { value: secretValue });
    await workspaceDashboard.netPolicyApplyReport.waitForExist({ timeout: SUBPROCESS });
    const report = await workspaceDashboard.applyReportText();
    assert(!report.includes(`$${ENV_VAR}`), `the apply report still lists a missing value: ${report}`);

    const r = guestCurl(name, `https://${ECHO_HOST}/headers`, {
      headers: [`Authorization: Bearer $${ENV_VAR}`],
    });
    assert.equal(r.code, "200", `${r.status} ${r.stderr}`);
    assert(r.body.includes(secretValue), `the upstream must see the real value: ${r.body}`);
    assert.equal(guestSh(name, `printenv ${ENV_VAR}`).stdout, PLACEHOLDER, "the guest must still hold the placeholder");
  });

  it("applies a policy change with a reload, not a restart", async () => {
    const startedAt = podman(LOCAL_CONNECTION, ["inspect", sidecar, "--format", "{{.State.StartedAt}}"]);

    await workspaceDashboard.addAllowRule("example.com");
    await workspaceDashboard.saveNetPolicy();
    // The report renders before the editor's refresh clears the drift flag.
    await workspaceDashboard.netPolicyDrift.waitForExist({
      reverse: true,
      timeout: COMMAND,
      timeoutMsg: "drift must clear after a successful apply",
    });

    const r = guestCurl(name, "https://example.com/");
    assert.equal(r.code, "200", `${r.status} ${r.stderr}`);

    assert.equal(
      podman(LOCAL_CONNECTION, ["inspect", sidecar, "--format", "{{.State.StartedAt}}"]),
      startedAt,
      "the sidecar restarted: a policy change must reload the running pipeline",
    );
    const saved = JSON.parse(fs.readFileSync(policyFile, "utf8"));
    assert(
      saved.rules.some((rule: { host?: string }) => rule.host === "example.com"),
      "the workspace policy file must hold the new rule",
    );
  });

  it("records the traffic, including the rejected request", async () => {
    await workspaceDashboard.openSection("traffic");
    await workspaceDashboard.waitForFlowHost(ECHO_HOST, { timeout: SUBPROCESS });
    // The earlier example.com request was refused before the rule was added; the ingest reads from
    // audit.cursor, so it still arrives when the panel starts watching late.
    await workspaceDashboard.waitForFlowHost("example.com", { rejected: true, timeout: SUBPROCESS });

    for (const file of ["flows.ndjson", "audit.cursor"]) {
      const p = path.join(stateDir, file);
      assert(fs.existsSync(p), `${file} is missing`);
      assert(fs.statSync(p).mtimeMs >= runStart, `${file} was not written during this run`);
    }
  });

  it("holds the secret value only in the secret store and the sidecar's secret file", () => {
    const needle = secretValue;
    const importHits = walk(workspaceDir).filter((f) => fs.readFileSync(f, "utf8").includes(needle));
    assert.deepEqual(importHits, [], "the value leaked into the workspace");

    for (const target of [name, sidecar]) {
      assert(!podman(LOCAL_CONNECTION, ["inspect", target]).includes(needle), `${target}'s inspect output holds the value`);
    }
    assert(!readFileIfExists(path.join(stateDir, "flows.ndjson")).includes(needle), "flows.ndjson holds the value");

    const logs = walk(path.join(APP_DATA_ROOT, "logs")).filter((f) => path.basename(f).startsWith("mio.log"));
    for (const f of logs) assert(!fs.readFileSync(f, "utf8").includes(needle), `${f} holds the value`);

    assert(readFileIfExists(path.join(APP_DATA_ROOT, "proxy", "secrets.json")).includes(needle), "secrets.json lost the value");
    assert.equal(
      fs.readFileSync(path.join(stateDir, "secrets", ENV_VAR), "utf8"),
      needle,
      "the sidecar secret file must hold exactly the value's bytes, with no trailing newline",
    );
  });

  it("removes the sidecar, the internal network and the state directory on destroy", async () => {
    const row = ProjectsPage.row(name);
    await row.destroy();
    await row.waitForGone(COMMAND);

    // The row disappears once the sandbox is removed; remove_proxy then takes the sidecar, the
    // network and the state directory, so poll instead of asserting at the moment the row vanishes.
    await browser.waitUntil(
      async () =>
        !podmanOk(LOCAL_CONNECTION, ["container", "exists", sidecar]) &&
        !podmanOk(LOCAL_CONNECTION, ["network", "exists", network]) &&
        !fs.existsSync(stateDir),
      { timeout: SUBPROCESS, timeoutMsg: "remove_proxy left the sidecar, the internal network or the state directory behind" },
    );
    assert(!podmanOk(LOCAL_CONNECTION, ["container", "exists", name]), "the sandbox container remains");
    assert(
      podmanOk(LOCAL_CONNECTION, ["network", "exists", "podman"]),
      "remove_proxy must never remove Podman's default network",
    );
  });
});
