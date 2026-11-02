import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

import ProjectsPage from "../../pageobjects/projects.page.js";
import { captureDiagOnFailure } from "../../../lib/diag.js";
import { MSB_PATH } from "../../../lib/run-identity.js";
import { BASE_IMAGE_BUILD, COMMAND, SUBPROCESS } from "../../../lib/timeouts.js";

// STANDARD spec for docs/features/microsandbox-network-policy: a real microsandbox VM created with
// the allowlist egress mode reaches a seeded host and cannot reach one that is not listed. The light
// spec only proves the rendered `msb create` argv; this proves microsandbox enforces it.
//
// Guest state is read through `msb exec`, which inherits the run's slug-keyed MSB_HOME from
// wdio.conf.ts, so it sees the same sandbox store as the app under test.
const WORKSPACE_ROOT = process.env.SANDBOX_WORKSPACE_ROOT ?? "";

function msb(args: string[]): string {
  return execFileSync(MSB_PATH, args, { encoding: "utf8", timeout: SUBPROCESS });
}

/** curl from inside the guest; `code` is the HTTP status, or `000` when no response came back. */
function guestCurl(name: string, url: string): { status: number | null; code: string; stderr: string } {
  const r = spawnSync(
    MSB_PATH,
    ["exec", name, "--", "curl", "-sS", "--max-time", "15", "-o", "/dev/null", "-w", "%{http_code}", url],
    { encoding: "utf8", timeout: SUBPROCESS },
  );
  return { status: r.status, code: r.stdout.trim(), stderr: r.stderr.trim() };
}

describe("Microsandbox network policy — allowlist egress on a real VM", function () {
  captureDiagOnFailure();
  this.timeout(BASE_IMAGE_BUILD + 5 * 60_000);

  const name = `e2e-egress-${Date.now()}`;

  it("seeds the allowlist, reaches an allowed host and is denied a non-listed one", async () => {
    await ProjectsPage.open();
    await ProjectsPage.ensurePodmanReady();
    await ProjectsPage.chooseProvider("microsandbox");
    await ProjectsPage.chooseEgress("allowlist");
    // The first microsandbox sandbox of a run builds the base image with Podman and `msb load`s it
    // into the fresh slug-keyed store, so the row can take minutes to appear.
    const row = await ProjectsPage.createProject(name, { timeout: BASE_IMAGE_BUILD });

    const policy = JSON.parse(fs.readFileSync(path.join(WORKSPACE_ROOT, name, ".mio", "net-policy.json"), "utf8"));
    expect(policy.egress).toBe("allowlist");
    const hosts = policy.rules.map((r: { host?: string }) => r.host);
    expect(hosts).toContain("registry.npmjs.org");
    expect(hosts).not.toContain("example.com");

    // `msb create` boots the VM, so there is no Start to click.
    await row.waitForRunning();

    const allowed = guestCurl(name, "https://registry.npmjs.org/");
    expect(`${allowed.status} ${allowed.code} ${allowed.stderr}`).toMatch(/^0 200 /);

    // Denied names do not resolve at microsandbox's gateway DNS (spike/01_egress-baseline.md), so
    // curl exits non-zero with no HTTP response rather than receiving a policy 403.
    const denied = guestCurl(name, "https://example.com/");
    expect(denied.status).not.toBe(0);
    expect(denied.code).toBe("000");

    await row.destroy();
    await row.waitForGone(COMMAND);
    expect(msb(["ls"])).not.toContain(name);
    expect(msb(["volume", "ls"])).not.toContain(`${name}-ws`);
  });
});
