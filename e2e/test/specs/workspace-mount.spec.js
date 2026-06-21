import fs from "fs";
import path from "path";
import { spawnSync } from "child_process";

// Mirrors the env wdio.conf.js passes to the app, so this spec resolves the same per-sandbox
// directory the backend writes.
const WORKSPACE_ROOT = process.env.SANDBOX_WORKSPACE_ROOT;
const CONNECTION = "mio-tests";

// Run a podman CLI command against the test machine and return trimmed stdout.
function podman(args) {
  const out = spawnSync("podman", ["--connection", CONNECTION, ...args], {
    encoding: "utf8",
    shell: true,
  });
  if (out.status !== 0) {
    throw new Error(`podman ${args.join(" ")} failed: ${out.stderr}`);
  }
  return out.stdout.trim();
}

describe("Sandbox workspace directory", () => {
  it("creates a host workspace dir bind-mounted at /root/work and shared with the container", async () => {
    // SANDBOX_WORKSPACE_ROOT must be set by wdio.conf.js.
    expect(WORKSPACE_ROOT).toBeTruthy();

    const projectName = `e2e-ws-${Date.now()}`;
    const workspaceDir = path.join(WORKSPACE_ROOT, projectName);

    const projectsBtn = await $("a[href='/projects']");
    await projectsBtn.waitForDisplayed({ timeout: 10000 });
    await projectsBtn.click();

    const nameInput = await $("input[placeholder='my-project']");
    await nameInput.setValue(projectName);
    const createBtn = await $("button=Create");
    await createBtn.click();

    const row = await $(`li*=${projectName}`);
    await row.waitForDisplayed({ timeout: 10000 });

    // 1. The host workspace directory exists on the laptop.
    expect(fs.existsSync(workspaceDir)).toBe(true);

    // 2. The container is wired with a bind mount at /root/work (inspectable before start).
    const mounts = JSON.parse(podman(["inspect", projectName, "--format", "json"]))[0].Mounts;
    // The container should have a mount at /root/work.
    const workMount = mounts.find((m) => m.Destination === "/root/work");
    expect(workMount).toBeDefined();

    // 3. The mount is live: a file written on the host is readable inside the container.
    const marker = "shared-from-host";
    fs.writeFileSync(path.join(workspaceDir, "marker.txt"), marker);

    const startBtn = await row.$("button=start");
    await startBtn.click();
    const greenDot = await row.$("span.bg-green-500");
    await greenDot.waitForExist({ timeout: 10000 });

    const seenInContainer = podman(["exec", projectName, "cat", "/root/work/marker.txt"]);
    expect(seenInContainer).toContain(marker);

    // Leave the machine clean for the next spec (10s SIGTERM grace + event delivery).
    const stopBtn = await row.$("button=stop");
    await stopBtn.scrollIntoView();
    await stopBtn.click();
    await greenDot.waitForExist({ timeout: 20000, reverse: true });
  });
});
