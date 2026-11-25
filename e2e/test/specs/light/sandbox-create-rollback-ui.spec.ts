import assert from "node:assert";
import fs from "node:fs";
import path from "node:path";

import ProjectsPage from "../../pageobjects/projects.page.js";
import { captureDiagOnFailure } from "../../../lib/diag.js";
import { configureMock, invokeCmd, invokeCmdExpectingError, mockCalls, useMockControl } from "../../../lib/mock.js";
import { APP_DATA_ROOT } from "../../../lib/run-identity.js";
import { sshAliasBlock } from "../../../lib/ssh.js";

// LIGHT spec (e2e/AGENTS.md decision rule 3): a failed key install cannot be provoked on demand
// against real Podman. Only `podman cp`'s process call is scripted; CliBackend::create's real
// rollback runs.
describe("Sandbox create — rollback after a failed key install (mock backend)", function () {
  this.timeout(60000);
  captureDiagOnFailure();
  useMockControl();

  const name = `e2e-create-rb-${Date.now()}`;
  const keyDir = path.join(APP_DATA_ROOT, "ssh-keys", name);
  const appliedDir = path.join(APP_DATA_ROOT, "net-policy", name);
  const proxyDir = path.join(APP_DATA_ROOT, "proxy", name);

  const listed = async () => (await invokeCmd<{ name: string }[]>("list_sandboxes")).map((s) => s.name);

  after(async () => {
    // Also strips the real ~/.ssh alias block the retry wrote.
    await invokeCmd("destroy_sandbox", { name, force: true }).catch(() => undefined);
  });

  it("removes the container, alias and key, so a same-name retry succeeds", async () => {
    await ProjectsPage.open();
    await ProjectsPage.ensurePodmanReady();
    await configureMock("podman.cp", { kind: "fail-once", exit_code: 1, stderr: "cp boom (scripted)" });

    const error = await invokeCmdExpectingError("create_sandbox", { name });
    assert.ok(error, "create_sandbox should have failed");
    assert.strictEqual(error.kind, "Backend");
    assert.match(error.message, /cp boom/);

    const removes = await mockCalls("podman.rm");
    assert.ok(
      removes.some((c) => c.argv.at(-1) === name),
      "the sandbox container itself was removed",
    );
    assert.ok(!(await listed()).includes(name), "the failed sandbox is not listed");
    assert.strictEqual(fs.existsSync(keyDir), false, "ssh-keys dir removed");
    assert.strictEqual(sshAliasBlock(name), undefined, "no ssh alias left");
    assert.strictEqual(fs.existsSync(appliedDir), false, "applied policy removed");
    assert.strictEqual(fs.existsSync(proxyDir), false, "proxy state removed");

    // The fail-once override was consumed, so the retry's cp succeeds by default.
    await invokeCmd("create_sandbox", { name });
    assert.ok((await listed()).includes(name), "the retry created the sandbox");
    assert.ok(fs.existsSync(keyDir), "retry installed the key");
    assert.notStrictEqual(sshAliasBlock(name), undefined, "retry wrote the alias");
  });
});
