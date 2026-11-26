import assert from "node:assert";
import { spawnSync } from "node:child_process";
import fs from "fs";
import path from "path";

import ProjectsPage from "../../pageobjects/projects.page.js";
import workspaceDashboard from "../../pageobjects/workspace-dashboard.page.js";
import { showHome } from "../../pageobjects/app.page.js";
import { captureDiagOnFailure } from "../../../lib/diag.js";
import { invokeCmd, mockCalls, useMockControl } from "../../../lib/mock.js";
import { E2E_SSH_DIR, SSH_DIR_SPEC_MARKER, readAliasBlock, sandboxAlias, sshAliasBlock } from "../../../lib/ssh.js";

// LIGHT spec: a sandbox named like a host in the user's SSH config must not take that host over
// (docs/features/namespaced-ssh-alias/02_dev.md). Its alias is `<name>.mio`, so the managed block never matches the real host.
// Runs against a per-run SSH directory (MIO_SSH_DIR, gated in e2e/wdio.conf.ts), never ~/.ssh; light because it needs a seeded user config.
// Not covered: the folder-mode suggestion that maps dots to dashes (CreateSandboxItem.svelte). The native dialog cannot be stubbed from the page: Tauri defines
// window.__TAURI_INTERNALS__ and its `invoke` as non-writable, non-configurable properties, so `plugin:dialog|open` cannot be intercepted.
describe("A sandbox named like an SSH host does not shadow it (mock backend)", function () {
  this.timeout(90000);

  useMockControl();
  captureDiagOnFailure();

  // The tests after the first share the sandbox it creates.
  const userConfig = path.join(E2E_SSH_DIR, "config");
  const managedConfig = path.join(E2E_SSH_DIR, "sandbox-mio", "config");
  // `ssh -F` reads this copy of the user config, run from E2E_SSH_DIR so the path stays relative.
  const probeName = "config.probe";
  const includeLine = "Include sandbox-mio/config";

  // Hosts the user already has; the sandbox below takes the name of the first one.
  const seededHost = "Work-Laptop";
  const seededConfig = [
    `Host ${seededHost}`,
    "    HostName 192.0.2.10",
    "    User alice",
    "Host github.com",
    "    HostName ssh.github.com",
    "    Port 443",
    "    User git",
    "Host *",
    "    ServerAliveInterval 60",
    "",
  ].join("\n");

  before(async () => {
    // Without the marker in this spec's path the harness does not set MIO_SSH_DIR, and the creates
    // below would write alias blocks into the developer's real ~/.ssh.
    assert.ok(
      import.meta.url.includes(SSH_DIR_SPEC_MARKER),
      "spec renamed: MIO_SSH_DIR gate in wdio.conf.ts no longer applies",
    );
    fs.rmSync(E2E_SSH_DIR, { recursive: true, force: true });
    fs.mkdirSync(path.join(E2E_SSH_DIR, "sandbox-mio"), { recursive: true });
    fs.writeFileSync(userConfig, seededConfig);
    await ProjectsPage.open();
    await ProjectsPage.ensurePodmanReady();
  });

  after(async () => {
    await invokeCmd("destroy_sandbox", { name: seededHost, force: true }).catch(() => undefined);
    fs.rmSync(E2E_SSH_DIR, { recursive: true, force: true });
  });

  /**
   * What `ssh -G <host>` resolves against the seeded user config with the managed config inlined at
   * its `Include` line. Inlined because ssh resolves a relative `Include` against the real ~/.ssh,
   * and Git's MSYS ssh cannot read the drive-letter path an absolute one would need. Makes no connection.
   */
  function sshResolve(host: string): Map<string, string> {
    const user = fs.readFileSync(userConfig, "utf8");
    const managed = fs.readFileSync(managedConfig, "utf8");
    assert.ok(user.includes(includeLine), "user config must include the managed config");
    fs.writeFileSync(path.join(E2E_SSH_DIR, probeName), user.replace(includeLine, () => managed));
    const out = spawnSync("ssh", ["-F", probeName, "-G", host], { cwd: E2E_SSH_DIR, encoding: "utf8" });
    assert.strictEqual(out.status, 0, `ssh -G ${host} failed: ${out.error ?? out.stderr}`);
    const resolved = new Map<string, string>();
    for (const line of out.stdout.split(/\r?\n/)) {
      const space = line.indexOf(" ");
      // The first occurrence wins, matching OpenSSH for the single-valued keywords read here.
      if (space > 0 && !resolved.has(line.slice(0, space))) resolved.set(line.slice(0, space), line.slice(space + 1));
    }
    return resolved;
  }

  it("creates a sandbox named like a seeded host and gives it only a .mio alias block", async () => {
    await ProjectsPage.createProject(seededHost);

    // The alias block lands under the relocated directory, never in the real ~/.ssh.
    const alias = sandboxAlias(seededHost);
    const block = readAliasBlock(alias, managedConfig);
    assert.ok(block?.includes(`Host ${alias}`), `no "Host ${alias}" block under MIO_SSH_DIR: ${block}`);
    assert.strictEqual(readAliasBlock(seededHost, managedConfig), undefined, "a bare-name block must not be written");
    assert.strictEqual(sshAliasBlock(seededHost), undefined, "the real ~/.ssh/sandbox-mio/config must hold no block for this run");
    assert.strictEqual(readAliasBlock(seededHost), undefined, "the real ~/.ssh/sandbox-mio/config must hold no bare block");
  });

  it("leaves the user config as it was apart from the managed Include", async () => {
    // The only edit the app may make is the leading Include, added once.
    assert.strictEqual(fs.readFileSync(userConfig, "utf8"), `${includeLine}\n\n${seededConfig}`);
  });

  it("keeps the seeded host resolving to itself and routes the .mio alias through the sandbox", async () => {
    const seeded = sshResolve(seededHost);
    assert.strictEqual(seeded.get("hostname"), "192.0.2.10");
    assert.strictEqual(seeded.get("user"), "alice");
    assert.strictEqual(seeded.get("proxycommand"), undefined, "the seeded host must not gain the sandbox's ProxyCommand");
    assert.strictEqual(sshResolve("github.com").get("hostname"), "ssh.github.com");

    // The managed block sits first, so its keywords win over the user's `Host *`.
    const sandbox = sshResolve(sandboxAlias(seededHost));
    const proxyCommand = sandbox.get("proxycommand") ?? "";
    assert.ok(
      proxyCommand.includes(`exec -i ${seededHost} mio-sshd`),
      `ProxyCommand does not exec into this sandbox: ${proxyCommand}`,
    );
    assert.strictEqual(sandbox.get("user"), "root");
    assert.strictEqual(sandbox.get("serveraliveinterval"), "15");
  });

  it("shows the derived alias in the SSH access panel", async () => {
    const row = ProjectsPage.row(seededHost);
    await row.waitForRunning();
    await row.openWorkspace();
    await workspaceDashboard.waitForRendered(seededHost);
    await workspaceDashboard.openSection("ports");
    assert.strictEqual(await workspaceDashboard.sshAlias(), sandboxAlias(seededHost));

    // Opening the panel leaves the block under the derived alias and adds no bare-name one, and the user config stays untouched.
    assert.ok(readAliasBlock(sandboxAlias(seededHost), managedConfig), "the .mio block must survive opening the panel");
    assert.strictEqual(readAliasBlock(seededHost, managedConfig), undefined);
    assert.strictEqual(fs.readFileSync(userConfig, "utf8"), `${includeLine}\n\n${seededConfig}`);
  });

  it("refuses a dotted name like a real host and creates nothing", async () => {
    const name = "github.com";
    await showHome();
    await (await ProjectsPage.nameInput).setValue(name);
    await (await ProjectsPage.createButton).click();

    const card = $(`li[data-sandbox="${name}"][data-pending="create"]`);
    await card.waitForDisplayed({ timeout: 30000 });
    const status = card.$("p[role='status']");
    await status.waitForDisplayed({ timeout: 30000, timeoutMsg: "refusal not shown" });
    // The exact text the create row shows: the raw `BackendError` message (`src-tauri/crates/domain/src/types.rs`).
    assert.strictEqual(
      await status.getText(),
      `invalid sandbox name "${name}": new names cannot contain '.', because Mutagen sync and forward session names are built from the sandbox name and Mutagen rejects dots`,
    );
    assert.strictEqual((await mockCalls("podman.create")).length, 0, "podman create must not run");
    assert.strictEqual(readAliasBlock(sandboxAlias(name), managedConfig), undefined, "a refused name must get no alias block");
    assert.strictEqual(fs.readFileSync(userConfig, "utf8"), `${includeLine}\n\n${seededConfig}`);
  });
});
