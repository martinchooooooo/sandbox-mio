import fs from "fs";
import path from "path";
import { spawnSync } from "child_process";

const WORKSPACE_ROOT = process.env.SANDBOX_WORKSPACE_ROOT;
const SOURCE_CONNECTION = "mio-tests";
const REMOTE_CONNECTION = "mio-tests-remote";

// Throws with a diagnostic message when condition is false. Used instead of expect().toBe(true)
// because withMessage() is only supported on async element matchers in expect-webdriverio, not
// on plain JS value assertions.
function assert(condition, message) {
  if (!condition) throw new Error(message);
}

// Run a podman command against a named connection. shell:false is critical: commands carry shell
// metacharacters in their remote-shell arguments and routing through cmd.exe would eat them.
function podman(connection, args, { check = true } = {}) {
  const out = spawnSync("podman", ["--connection", connection, ...args], {
    encoding: "utf8",
    shell: false,
  });
  if (check && out.status !== 0) {
    throw new Error(
      `podman --connection ${connection} ${args.join(" ")} failed: ${out.stderr}`
    );
  }
  return out.stdout?.trim() ?? "";
}

// Returns true if the command exits 0 (container/image exists), false otherwise.
function podmanOk(connection, args) {
  return (
    spawnSync("podman", ["--connection", connection, ...args], { shell: false }).status === 0
  );
}

// Poll the deployment status text in the project row until it equals the target value.
// The status span sits inside the row next to the host label: both share text-muted-foreground
// and text-xs, but the status span does not have shrink-0.
async function waitForSyncStatus(projectName, targetStatus, timeoutMs = 60000) {
  await browser.waitUntil(
    async () => {
      const row = await $(`li*=${projectName}`);
      const spans = await row.$$("span.text-muted-foreground.text-xs");
      for (const span of spans) {
        if ((await span.getText()) === targetStatus) return true;
      }
      return false;
    },
    {
      timeout: timeoutMs,
      interval: 2000,
      timeoutMsg: `sync status did not reach "${targetStatus}" within ${timeoutMs}ms`,
    }
  );
}

describe("Deploy sandbox to remote machine", () => {
  const projectName = `e2e-deploy-${Date.now()}`;
  let workspaceDir;

  before(() => {
    expect(WORKSPACE_ROOT).toBeTruthy();
    workspaceDir = path.join(WORKSPACE_ROOT, projectName);
  });

  after(() => {
    // Best-effort cleanup so a failed run does not leave containers behind.
    spawnSync(
      "podman",
      ["--connection", SOURCE_CONNECTION, "rm", "-f", projectName],
      { shell: false }
    );
    spawnSync(
      "podman",
      ["--connection", REMOTE_CONNECTION, "rm", "-f", projectName],
      { shell: false }
    );
  });

  it("creates a local project and writes a marker file to its workspace", async () => {
    const projectsBtn = await $("a[href='/projects']");
    await projectsBtn.waitForDisplayed({ timeout: 10000 });
    await projectsBtn.click();

    const nameInput = await $("input[placeholder='my-project']");
    await nameInput.setValue(projectName);
    const createBtn = await $("button=Create");
    await createBtn.click();

    const row = await $(`li*=${projectName}`);
    await row.waitForDisplayed({ timeout: 10000 });

    fs.writeFileSync(path.join(workspaceDir, "marker.txt"), "laptop-marker");
    assert(
      fs.existsSync(path.join(workspaceDir, "marker.txt")),
      `Unable to write marker file to ${workspaceDir}. This can happen when ` +
        "create_sandbox did not create the workspace directory (check SANDBOX_WORKSPACE_ROOT is set " +
        "and the backend's create_dir_all succeeded), or when the path is not writable."
    );
  });

  it("deploys to mio-tests-remote and the remote container starts", async () => {
    const row = await $(`li*=${projectName}`);

    const deployBtn = await row.$("button=deploy");
    await deployBtn.scrollIntoView();
    await deployBtn.click();

    const hostSelect = await row.$("select");
    await hostSelect.waitForDisplayed({ timeout: 5000 });
    await hostSelect.selectByVisibleText(REMOTE_CONNECTION);

    const confirmBtn = await row.$("button=Deploy");
    await confirmBtn.scrollIntoView();
    await confirmBtn.click();

    const undeployBtn = await row.$("button=undeploy");
    await undeployBtn.waitForDisplayed({ timeout: 120000 });

    const state = podman(REMOTE_CONNECTION, [
      "inspect",
      projectName,
      "--format",
      "{{.State.Status}}",
    ]);
    assert(
      state === "running",
      `Remote container "${projectName}" on ${REMOTE_CONNECTION} is in state "${state}" after deploy, expected "running". ` +
        "This can happen when the image save/load succeeded but podman create or start failed on the target connection."
    );

    assert(
      await row.$(`span*=${REMOTE_CONNECTION}`).isDisplayed(),
      `Deployed host label "${REMOTE_CONNECTION}" not visible in the project row. ` +
        "This can happen when sandbox_deployments did not return the session, or the frontend did not re-poll after deploy."
    );
  });

  it("marker file written on the laptop appears inside the remote container", async () => {
    await waitForSyncStatus(projectName, "up-to-date");

    const content = podman(REMOTE_CONNECTION, [
      "exec",
      projectName,
      "cat",
      "/root/work/marker.txt",
    ]);
    assert(
      content === "laptop-marker",
      `marker.txt in the remote container contained "${content}" instead of "laptop-marker". ` +
        "This can happen when the Mutagen session was created but the initial sync did not propagate before the assertion, " +
        "or when the remote workspace path differs from the container's bind-mount source."
    );
  });

  it("file written inside the remote container lands in the laptop directory", async () => {
    podman(REMOTE_CONNECTION, [
      "exec",
      projectName,
      "sh",
      "-c",
      "echo from-remote > /root/work/from-remote.txt",
    ]);

    await browser.waitUntil(
      () => fs.existsSync(path.join(workspaceDir, "from-remote.txt")),
      {
        timeout: 30000,
        interval: 1000,
        timeoutMsg: "remote file did not appear in the laptop workspace directory",
      }
    );

    const content = fs
      .readFileSync(path.join(workspaceDir, "from-remote.txt"), "utf8")
      .trim();
    assert(
      content === "from-remote",
      `from-remote.txt contained "${content}" instead of "from-remote". ` +
        "This can happen when the echo command wrote extra whitespace or a shell prompt polluted stdout — " +
        "check that the exec used sh -c and that the remote shell is non-interactive."
    );
  });

  it("sync toggle off and on leaves the session alive", async () => {
    const row = await $(`li*=${projectName}`);
    const syncSwitch = await row.$("button[role='switch']");
    await syncSwitch.scrollIntoView();

    await syncSwitch.click(); // pause
    await browser.pause(500);
    await syncSwitch.click(); // resume

    assert(
      await row.$("button=undeploy").isDisplayed(),
      "Undeploy button disappeared after toggling sync off then on. " +
        "This can happen when pause or resume returned an error and the frontend erroneously dropped the deployment state."
    );
  });

  it("undeploy removes the remote container and restarts the local one", async () => {
    const row = await $(`li*=${projectName}`);
    const undeployBtn = await row.$("button=undeploy");
    await undeployBtn.scrollIntoView();
    await undeployBtn.click();

    const deployBtn = await row.$("button=deploy");
    await deployBtn.waitForDisplayed({ timeout: 30000 });

    assert(
      !podmanOk(REMOTE_CONNECTION, ["inspect", projectName]),
      `Remote container "${projectName}" still exists on ${REMOTE_CONNECTION} after undeploy. ` +
        "This can happen when remove_remote failed silently or the wrong connection name was used."
    );

    const localState = podman(SOURCE_CONNECTION, [
      "inspect",
      projectName,
      "--format",
      "{{.State.Status}}",
    ]);
    assert(
      ["running", "exited", "created"].includes(localState),
      `Local container "${projectName}" is in state "${localState}" after undeploy, which is not a valid post-undeploy state. ` +
        "This can happen when the local container was removed rather than restarted, or when start failed after the session was torn down."
    );
  });

  it("destroy removes the local container but leaves the workspace directory", async () => {
    const row = await $(`li*=${projectName}`);
    const destroyBtn = await row.$("button=destroy");
    await destroyBtn.scrollIntoView();
    await destroyBtn.click();

    await row.waitForExist({ timeout: 15000, reverse: true });

    assert(
      !podmanOk(SOURCE_CONNECTION, ["inspect", projectName]),
      `Local container "${projectName}" still exists on ${SOURCE_CONNECTION} after destroy. ` +
        "This can happen when destroy_sandbox returned an error that the frontend swallowed, or when the wrong container name was targeted."
    );

    assert(
      fs.existsSync(workspaceDir),
      `Workspace directory ${workspaceDir} was deleted by destroy. ` +
        "destroy_sandbox must remove the container but leave the workspace intact so the user's files are not lost."
    );
  });
});
