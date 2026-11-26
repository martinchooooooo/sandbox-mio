import assert from "node:assert";
import fs from "fs";
import path from "path";

import ProjectsPage from "../../pageobjects/projects.page.js";
import workspaceDashboard from "../../pageobjects/workspace-dashboard.page.js";
import { showHome } from "../../pageobjects/app.page.js";
import { captureDiagOnFailure } from "../../../lib/diag.js";
import { configureMock, mockCalls, useMockControl } from "../../../lib/mock.js";
import { MISE_BASE_IMAGE } from "../../../lib/podman-machine.js";
import { E2E_SSH_DIR, SSH_DIR_SPEC_MARKER, sshAliasBlock } from "../../../lib/ssh.js";

// LIGHT spec: a new sandbox name must not shadow the user's SSH hosts (docs/features/host-tool-ssh-access/04_improvements.md).
// Runs against a per-run SSH directory (MIO_SSH_DIR, gated in e2e/wdio.conf.ts), never ~/.ssh; light because pure validation needs a seeded user config.
// Not covered: the folder-mode suggestion that maps dots to dashes (CreateSandboxItem.svelte). The native dialog cannot be stubbed from the page: Tauri defines
// window.__TAURI_INTERNALS__ and its `invoke` as non-writable, non-configurable properties, so `plugin:dialog|open` cannot be intercepted.
describe("New sandbox names cannot shadow SSH hosts (mock backend)", function () {
  this.timeout(90000);

  useMockControl();
  captureDiagOnFailure();

  const ts = Date.now();
  const placeholder = (name: string) => $(`li[data-sandbox="${name}"][data-pending="create"]`);
  const userConfig = path.join(E2E_SSH_DIR, "config");
  const managedConfig = path.join(E2E_SSH_DIR, "sandbox-mio", "config");
  const leftover = `e2e-leftover-${ts}`;

  // The exact text the create row shows: the raw `BackendError` message (`src-tauri/src/backends/types.rs`).
  const shadowsHost = (name: string, file: string, pattern: string) =>
    `invalid sandbox name "${name}": your SSH config ${file} already has a "Host" entry matching "${pattern}", and this sandbox's SSH alias would take that entry over; choose a different name`;

  before(async () => {
    // Without the marker in this spec's path the harness does not set MIO_SSH_DIR, and the creates
    // below would write alias blocks into the developer's real ~/.ssh.
    assert.ok(
      import.meta.url.includes(SSH_DIR_SPEC_MARKER),
      "spec renamed: MIO_SSH_DIR gate in wdio.conf.ts no longer applies",
    );
    fs.rmSync(E2E_SSH_DIR, { recursive: true, force: true });
    fs.mkdirSync(path.join(E2E_SSH_DIR, "conf.d"), { recursive: true });
    fs.mkdirSync(path.join(E2E_SSH_DIR, "sandbox-mio"), { recursive: true });
    fs.writeFileSync(
      userConfig,
      [
        "Host Work-Laptop",
        "    HostName 192.0.2.10",
        "Host team-* !team-ci",
        "    User deploy",
        "Host *",
        "    ServerAliveInterval 60",
        "Match host matched-only",
        "    User nobody",
        "Include conf.d/*",
        "",
      ].join("\n"),
    );
    fs.writeFileSync(path.join(E2E_SSH_DIR, "conf.d", "extra"), "Host included-box\n");
    // A block a past create left behind, in the managed file only: the managed file is not part of
    // the user's config, so a new sandbox of this name must still be accepted.
    fs.writeFileSync(
      managedConfig,
      `# >>> sandbox-mio ${leftover} >>>\nHost ${leftover}\n    User root\n# <<< sandbox-mio ${leftover} <<<\n`,
    );
    await ProjectsPage.open();
    await ProjectsPage.ensurePodmanReady();
  });

  after(() => {
    fs.rmSync(E2E_SSH_DIR, { recursive: true, force: true });
  });

  async function submit(name: string) {
    await showHome();
    await (await ProjectsPage.nameInput).setValue(name);
    await (await ProjectsPage.createButton).click();
  }

  /** Creates through the form, expects the placeholder to show exactly `expected`, and dismisses it. */
  async function expectRefusal(name: string, expected: string) {
    await submit(name);
    const card = placeholder(name);
    await card.waitForDisplayed({ timeout: 30000 });
    const status = card.$("p[role='status']");
    await status.waitForDisplayed({ timeout: 30000, timeoutMsg: "refusal not shown" });
    assert.strictEqual(await status.getText(), expected);
    assert.strictEqual((await mockCalls("podman.create")).length, 0, "podman create must not run");
    await card.$("[data-create-dismiss]").click();
    await card.waitForExist({ reverse: true, timeout: 5000 });
  }

  it("refuses a dotted name with a clear error and creates nothing", async () => {
    const name = `e2e.dotted-${ts}`;
    await expectRefusal(
      name,
      `invalid sandbox name "${name}": new names cannot contain '.', because the name becomes an SSH host alias and a dotted name could take over a real host such as github.com`,
    );
  });

  it("refuses a name matching a Host entry in the user's SSH config", async () => {
    // Differs in case from the fixture on purpose: matching is case-insensitive.
    await expectRefusal("work-laptop", shadowsHost("work-laptop", userConfig, "Work-Laptop"));

    // The pattern is quoted as written, and the file is the one that holds it.
    await expectRefusal("team-web", shadowsHost("team-web", userConfig, "team-*"));

    // The reported file is the included one, not `config`, which is what the user needs to read.
    await expectRefusal(
      "included-box",
      shadowsHost("included-box", path.join(E2E_SSH_DIR, "conf.d", "extra"), "included-box"),
    );
  });

  it("still creates a name covered only by a catch-all, negated, Match or managed-file entry", async () => {
    const name = `e2e-shadow-ok-${ts}`;
    await ProjectsPage.createProject(name);

    // The alias block lands under the relocated directory, never in the real ~/.ssh.
    const managed = fs.readFileSync(managedConfig, "utf8");
    assert.ok(managed.includes(`# >>> sandbox-mio ${name} >>>`), "alias block must land under MIO_SSH_DIR");
    assert.strictEqual(sshAliasBlock(name), undefined, "the real ~/.ssh/sandbox-mio/config must hold no block for this run");
    const user = fs.readFileSync(userConfig, "utf8");
    assert.ok(user.startsWith("Include sandbox-mio/config"), user);
    assert.ok(user.includes("Host Work-Laptop"), "user config must survive");

    // `team-ci` matches `team-*` but is vetoed by `!team-ci`; `matched-only` appears only in a
    // `Match` block. Neither is a `Host` clash.
    await ProjectsPage.createProject("team-ci");
    await ProjectsPage.createProject("matched-only");

    // The managed include now sits at the top of the user config and holds the leftover block, yet
    // that file is not scanned: a leftover alias is not a clash with the user's own hosts.
    await ProjectsPage.createProject(leftover);

    for (const created of [name, "team-ci", "matched-only", leftover]) {
      assert.strictEqual(sshAliasBlock(created), undefined, `real ~/.ssh holds a block for ${created}`);
    }
  });

  it("keeps a pre-existing dotted sandbox addressable", async () => {
    const name = `legacy.box-${ts}`;
    await configureMock("podman.ps", {
      kind: "return-value",
      stdout: JSON.stringify([
        {
          Id: "legacy-dotted-id",
          Names: [name],
          State: "running",
          Labels: { "sandbox-mio.workspace": `/tmp/${name}` },
          Image: MISE_BASE_IMAGE,
        },
      ]),
    });
    await ProjectsPage.open();
    const row = ProjectsPage.row(name);
    await row.waitForDisplayed();
    await row.openWorkspace();
    await workspaceDashboard.waitForRendered(name);
    await workspaceDashboard.openSection("ports");
    assert.strictEqual(await workspaceDashboard.sshAlias(), name);

    // Resolving the sandbox rewrites its alias (`ensure_forward_alias`, `src-tauri/src/services/routing.rs`),
    // so a dotted alias is still written for a sandbox that predates the rule.
    const managed = fs.readFileSync(managedConfig, "utf8");
    assert.ok(managed.includes(`# >>> sandbox-mio ${name} >>>`), "dotted alias block must be written");
    assert.strictEqual(sshAliasBlock(name), undefined, "the real ~/.ssh/sandbox-mio/config must hold no block for this run");
  });
});
