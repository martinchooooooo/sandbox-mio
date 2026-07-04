import assert from "node:assert";
import fs from "fs";
import path from "path";

import ProjectsPage from "../../pageobjects/projects.page.js";
import { podman, podmanOk, LOCAL_CONNECTION, REMOTE_CONNECTION } from "../../../lib/podman-connection.js";
import { useSandboxWorkspace } from "../../../lib/sandbox-suite.js";

// node:assert is used instead of expect().toBe(true) because expect-webdriverio's withMessage()
// is only supported on async element matchers, not on plain JS value assertions — node:assert's
// second argument carries the diagnostic message for free.

describe("Deploy sandbox to remote machine", () => {
  const { projectName, row, workspaceDir } = useSandboxWorkspace("e2e-deploy");

  after(() => {
    // Best-effort cleanup so a failed run does not leave the remote container behind (the local
    // one is already handled by useSandboxWorkspace's teardown).
    podman(REMOTE_CONNECTION, ["rm", "-f", projectName], { check: false });
  });

  it("creates a local project and writes a marker file to its workspace", async () => {
    await ProjectsPage.open();
    await ProjectsPage.createProject(projectName);

    fs.writeFileSync(path.join(workspaceDir, "marker.txt"), "laptop-marker");
    assert(
      fs.existsSync(path.join(workspaceDir, "marker.txt")),
      `Unable to write marker file to ${workspaceDir}. This can happen when ` +
        "create_sandbox did not create the workspace directory (check SANDBOX_WORKSPACE_ROOT is set " +
        "and the backend's create_dir_all succeeded), or when the path is not writable."
    );
  });

  it("deploys to mio-tests-remote and the remote container starts", async () => {
    await row.deployTo(REMOTE_CONNECTION);

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
      await row.isHostShown(REMOTE_CONNECTION),
      `Deployed host label "${REMOTE_CONNECTION}" not visible in the project row. ` +
        "This can happen when sandbox_deployments did not return the session, or the frontend did not re-poll after deploy."
    );
  });

  it("marker file written on the laptop appears inside the remote container", async () => {
    await row.waitForSyncStatus("up-to-date");

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
    await row.toggleSyncOffOn();

    assert(
      await row.isDeployed(),
      "Undeploy button disappeared after toggling sync off then on. " +
        "This can happen when pause or resume returned an error and the frontend erroneously dropped the deployment state."
    );
  });

  it("undeploy removes the remote container and restarts the local one", async () => {
    await row.undeploy();

    assert(
      !podmanOk(REMOTE_CONNECTION, ["inspect", projectName]),
      `Remote container "${projectName}" still exists on ${REMOTE_CONNECTION} after undeploy. ` +
        "This can happen when remove_remote failed silently or the wrong connection name was used."
    );

    const localState = podman(LOCAL_CONNECTION, [
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
    await row.destroy();

    assert(
      !podmanOk(LOCAL_CONNECTION, ["inspect", projectName]),
      `Local container "${projectName}" still exists on ${LOCAL_CONNECTION} after destroy. ` +
        "This can happen when destroy_sandbox returned an error that the frontend swallowed, or when the wrong container name was targeted."
    );

    assert(
      fs.existsSync(workspaceDir),
      `Workspace directory ${workspaceDir} was deleted by destroy. ` +
        "destroy_sandbox must remove the container but leave the workspace intact so the user's files are not lost."
    );
  });
});
