import fs from "fs";
import path from "path";

import ProjectsPage from "../../pageobjects/projects.page.js";
import { podman, LOCAL_CONNECTION } from "../../../lib/podman-connection.js";
import { captureDiagOnFailure } from "../../../lib/diag.js";

// Mirrors the env wdio.conf.ts passes to the app, so this spec resolves the same per-sandbox
// directory the backend writes.
const WORKSPACE_ROOT = process.env.SANDBOX_WORKSPACE_ROOT;

interface ContainerMount {
  Destination: string;
}

describe("Sandbox workspace directory", () => {
  captureDiagOnFailure();

  it("creates a host workspace dir bind-mounted at /root/work and shared with the container", async () => {
    // SANDBOX_WORKSPACE_ROOT must be set by wdio.conf.ts.
    expect(WORKSPACE_ROOT).toBeTruthy();

    const projectName = `e2e-ws-${Date.now()}`;
    const workspaceDir = path.join(WORKSPACE_ROOT!, projectName);

    await ProjectsPage.open();
    const row = await ProjectsPage.createProject(projectName);

    expect(fs.existsSync(workspaceDir)).toBe(true);

    // The bind mount at /root/work is inspectable before the container even starts.
    const mounts: ContainerMount[] = JSON.parse(
      podman(LOCAL_CONNECTION, ["inspect", projectName, "--format", "json"])
    )[0].Mounts;
    const workMount = mounts.find((m) => m.Destination === "/root/work");
    expect(workMount).toBeDefined();

    // Prove the mount is live: a file written on the host must be readable inside the container.
    const marker = "shared-from-host";
    fs.writeFileSync(path.join(workspaceDir, "marker.txt"), marker);

    await row.start();
    await row.waitForRunning();

    const seenInContainer = podman(LOCAL_CONNECTION, [
      "exec",
      projectName,
      "cat",
      "/root/work/marker.txt",
    ]);
    expect(seenInContainer).toContain(marker);

    // Leave the machine clean for the next spec (10s SIGTERM grace + event delivery).
    await row.stop();
    await row.waitForStopped();
  });
});
