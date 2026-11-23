import assert from "node:assert";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

import ProjectsPage from "../../pageobjects/projects.page.js";
import workspaceDashboard from "../../pageobjects/workspace-dashboard.page.js";
import { captureDiagOnFailure } from "../../../lib/diag.js";
import { invokeCmd } from "../../../lib/mock.js";
import { LOCAL_CONNECTION, podman } from "../../../lib/podman-connection.js";
import { APP_DATA_ROOT } from "../../../lib/run-identity.js";
import { BASE_IMAGE_BUILD, COMMAND, PACKAGE_INSTALL, PANEL_RENDER, SUBPROCESS } from "../../../lib/timeouts.js";

// STANDARD spec for docs/features/proxy-credential-secrets (03_e2e.md): a Podman sandbox behind an
// iron-proxy sidecar holds only the GitHub placeholder, and the sidecar swaps it for the real token on
// GitHub's hosts. A mock backend has no sidecar, so only a real one shows the swap, the offline refresh
// of a stopped sidecar and the token's absence from the guest.
//
// Token handling: the token reaches the app only through `set_github_token`. It is never logged, put
// in an assertion message, written to a file or typed into an input, because `captureDiagOnFailure()`
// dumps page source. Guest scans return counts, and every comparison that involves the token is a
// boolean `assert(cond, "fixed message")`, never `assert.equal(a, b)`, which prints both operands.
//
// MIO_E2E_GITHUB_TOKEN        read-only token (no write scopes); the spec skips without it.
// MIO_E2E_GITHUB_TOKEN_ALT    optional second token for the rotation step (the same token otherwise).
// MIO_E2E_GITHUB_REPO         optional public repo; defaults to github/gitignore, which has a README.md
//                             on its default branch. Only GETs and `git ls-remote` ever reach GitHub.

const TOKEN = process.env.MIO_E2E_GITHUB_TOKEN?.trim() ?? "";
const TOKEN_ALT = process.env.MIO_E2E_GITHUB_TOKEN_ALT?.trim() || TOKEN;
const REPO = process.env.MIO_E2E_GITHUB_REPO?.trim() || "github/gitignore";
// Mirrors GITHUB_PLACEHOLDER in src-tauri/src/credentials/github.rs.
const PLACEHOLDER = "ghp_MioPlaceholder0000000000000000000000";
const HOSTS_YML = "/root/.config/gh/hosts.yml";
// A tool resolved through mise's aqua backend, which reads its release list from GitHub.
const AQUA_TOOL = "aqua:BurntSushi/ripgrep";

interface Run {
  status: number | null;
  stdout: string;
  stderr: string;
}

interface AuditRow {
  host: string;
  path: string;
  status: number | null;
  swapped: boolean;
}

/** `podman exec`, optionally feeding `input` on stdin so a token never appears in an argument list. */
function guestSh(name: string, script: string, input?: string): Run {
  const r = spawnSync("podman", ["--connection", LOCAL_CONNECTION, "exec", ...(input === undefined ? [] : ["-i"]), name, "sh", "-c", script], {
    encoding: "utf8",
    timeout: PACKAGE_INSTALL,
    input,
  });
  return { status: r.status, stdout: r.stdout.trim(), stderr: r.stderr.trim() };
}

/** curl inside the sandbox; the status code follows the body on its own line (`000` = no response). */
function guestCurl(name: string, url: string, headers: string[] = []): { code: string; body: string; status: number | null } {
  const flags = ["-sS", "--max-time 20", ...headers.map((h) => `-H '${h}'`), `-w '\\n%{http_code}'`].join(" ");
  const r = guestSh(name, `curl ${flags} ${url}`);
  const lines = r.stdout.split("\n");
  const code = lines.pop() ?? "";
  return { code, body: lines.join("\n"), status: r.status };
}

/**
 * Files under /root plus process environments that hold `token`, as "<files> <env-matches>". Counts
 * only: matching lines would leak the token into the test log.
 */
function leakCount(name: string, token: string): { files: number; env: number } {
  const script =
    'read -r t; f=$(grep -rIlF -- "$t" /root 2>/dev/null | wc -l); ' +
    'e=$(cat /proc/[0-9]*/environ 2>/dev/null | tr "\\0" "\\n" | grep -cF -- "$t"); echo "$f $e"';
  const [files, env] = guestSh(name, script, `${token}\n`).stdout.split(" ").map(Number);
  return { files, env };
}

/** Whether the guest's hosts.yml contains `needle`; the needle may be the token, so only a boolean leaves. */
function hostsYmlHolds(name: string, needle: string): boolean {
  return guestSh(name, `cat ${HOSTS_YML} 2>/dev/null`).stdout.includes(needle);
}

function ghApiUser(name: string): Run {
  return guestSh(name, "gh api user --jq .login");
}

/** The sidecar's audit stream, one row per proxied request (the same JSON the Traffic panel ingests). */
function auditRows(sidecar: string): AuditRow[] {
  const logs = spawnSync("podman", ["--connection", LOCAL_CONNECTION, "logs", sidecar], { encoding: "utf8", timeout: SUBPROCESS });
  const rows: AuditRow[] = [];
  for (const line of `${logs.stdout}\n${logs.stderr}`.split("\n")) {
    const start = line.indexOf("{");
    if (start < 0) continue;
    try {
      const rec = JSON.parse(line.slice(start));
      if (rec.msg !== "request" || !rec.audit?.host) continue;
      const secrets = (rec.request_transforms ?? []).find((t: { name: string }) => t.name === "secrets");
      rows.push({
        host: String(rec.audit.host).replace(/:\d+$/, ""),
        path: String(rec.audit.path ?? ""),
        status: rec.audit.status_code ? Number(rec.audit.status_code) : null,
        swapped: Array.isArray(secrets?.annotations?.swapped) && secrets.annotations.swapped.length > 0,
      });
    } catch {
      // A line that is not audit JSON (startup banner, truncated write) carries no row.
    }
  }
  return rows;
}

/** Rows for `host` logged after the first `before` rows of that host, waiting for at least one. */
async function newRowsFor(sidecar: string, host: string, before: number): Promise<AuditRow[]> {
  let rows: AuditRow[] = [];
  await browser.waitUntil(
    async () => {
      // A CONNECT row has no status code; the inner request's row is the one that carries the marker.
      rows = auditRows(sidecar).filter((r) => r.host === host && r.status !== null).slice(before);
      return rows.length > 0;
    },
    { timeout: SUBPROCESS, timeoutMsg: `no audit row for ${host} appeared in the sidecar log` },
  );
  return rows;
}

const rowCount = (sidecar: string, host: string): number =>
  auditRows(sidecar).filter((r) => r.host === host && r.status !== null).length;

describe("GitHub placeholder swap — sidecar sandbox on a real machine", function () {
  captureDiagOnFailure();
  this.timeout(BASE_IMAGE_BUILD + PACKAGE_INSTALL + 20 * 60_000);

  const stamp = Date.now();
  const A = `e2e-gps-${stamp}`;
  const sidecar = `${A}-proxy`;
  const tokenFile = path.join(APP_DATA_ROOT, "proxy", A, "secrets", "mio-github");
  const card = () => $("[data-github-access]");

  before(function () {
    if (!TOKEN) {
      console.log("SKIPPED github-placeholder-swap: MIO_E2E_GITHUB_TOKEN is unset (a read-only GitHub token is required).");
      this.skip();
    }
  });

  after(async () => {
    // The credential store lives under the run's isolated app-data root; clear it and the sandbox.
    await invokeCmd("delete_credential_bundle", { serviceId: "github" }).catch(() => undefined);
    await invokeCmd("destroy_sandbox", { name: A, force: true }).catch(() => undefined);
  });

  async function waitForCardState(state: string): Promise<void> {
    await browser.waitUntil(async () => (await card().isExisting()) && (await card().getAttribute("data-state")) === state, {
      timeout: SUBPROCESS,
      timeoutMsg: `the GitHub card never reached data-state="${state}"`,
    });
  }

  /** The reconcile has finished applying the placeholder, judged host-side. */
  async function waitForApplied(name: string): Promise<void> {
    await browser.waitUntil(
      async () => {
        const s = await invokeCmd<{ outcome: { kind: string } | null; optedOut: boolean }>(
          "github_sandbox_status",
          { name },
        );
        return !s.optedOut && s.outcome?.kind === "applied";
      },
      { timeout: SUBPROCESS, timeoutMsg: "the GitHub login was never applied to the sandbox" },
    );
  }

  /** Polls until `mio-github` satisfies `want` (a bytes match, or null for deleted). */
  async function waitForTokenFile(want: string | null): Promise<void> {
    await browser.waitUntil(
      async () => {
        const exists = fs.existsSync(tokenFile);
        return want === null ? !exists : exists && fs.readFileSync(tokenFile, "utf8") === want;
      },
      { timeout: SUBPROCESS, timeoutMsg: want === null ? "mio-github was not deleted" : "mio-github did not take the expected bytes" },
    );
  }

  async function waitForGhApiUser(name: string): Promise<void> {
    await browser.waitUntil(async () => ghApiUser(name).status === 0, {
      timeout: SUBPROCESS,
      timeoutMsg: "gh api user did not succeed through the sidecar",
    });
  }

  async function openOverview(name: string): Promise<void> {
    await ProjectsPage.row(name).openWorkspace();
    await workspaceDashboard.waitForRendered(name);
    await workspaceDashboard.openSection("overview");
    await card().waitForExist({ timeout: PANEL_RENDER });
  }

  it("1. creates sandbox A behind an iron-proxy sidecar", async () => {
    await ProjectsPage.open();
    await ProjectsPage.ensurePodmanReady();
    // Open egress already reaches GitHub's hosts, so no policy needs seeding for this spec.
    const row = await ProjectsPage.createProject(A, { timeout: PACKAGE_INSTALL });
    await row.waitForRunning();

    const meta = JSON.parse(fs.readFileSync(path.join(APP_DATA_ROOT, "proxy", A, "meta.json"), "utf8"));
    assert.equal(meta.engine, "iron-proxy");
    const names = podman(LOCAL_CONNECTION, ["ps", "--format", "{{.Names}}"]).split("\n");
    assert(names.includes(sidecar), "the iron-proxy sidecar is not listed by podman ps");
  });

  it("2. saves the token and the card says it is kept securely", async () => {
    await invokeCmd("set_github_token", { token: TOKEN });
    await openOverview(A);
    await waitForCardState("applied");
    const text = await $("[data-github-access-text]").getText();
    assert(text.includes("token kept securely"), "the card must say the token is kept securely");
    assert(!text.includes("token stored in sandbox"), "the card must not say the token is stored in the sandbox");
  });

  it("3. leaves only the placeholder in the guest", () => {
    assert(hostsYmlHolds(A, PLACEHOLDER), "hosts.yml must hold the placeholder");
    const leaks = leakCount(A, TOKEN);
    assert(leaks.files === 0, "the token was found in a file under /root");
    assert(leaks.env === 0, "the token was found in a process environment");
    assert.equal(guestSh(A, "gh auth token").stdout, PLACEHOLDER);
  });

  it("4. swaps the placeholder on api.github.com and github.com", () => {
    const user = ghApiUser(A);
    assert.equal(user.status, 0, `gh api user failed: ${user.stderr}`);
    const lsRemote = guestSh(A, `GIT_TERMINAL_PROMPT=0 git ls-remote https://github.com/${REPO}.git HEAD`);
    assert.equal(lsRemote.status, 0, `git ls-remote failed: ${lsRemote.stderr}`);
    assert(fs.readFileSync(tokenFile, "utf8") === TOKEN, "mio-github must hold exactly the token's bytes");
  });

  it("5. swaps on hosts the spike left unproven, and only in Authorization", async () => {
    const raw = "raw.githubusercontent.com";
    const rawUrl = `https://${raw}/${REPO}/HEAD/README.md`;

    let before = rowCount(sidecar, raw);
    const bound = guestCurl(A, rawUrl, [`Authorization: Bearer ${PLACEHOLDER}`]);
    // An unswapped placeholder gets 404 from this host, so 200 plus the marker is the proof.
    assert.equal(bound.code, "200", `raw GET with the placeholder returned ${bound.code}`);
    assert((await newRowsFor(sidecar, raw, before)).some((r) => r.swapped), "the raw request's audit row lacks the swap marker");

    before = rowCount(sidecar, raw);
    guestCurl(A, rawUrl, [`X-Other: ${PLACEHOLDER}`]);
    assert(!(await newRowsFor(sidecar, raw, before)).some((r) => r.swapped), "a placeholder outside Authorization must not be swapped");

    const gist = "gist.github.com";
    before = rowCount(sidecar, gist);
    guestCurl(A, `https://${gist}/`, [`Authorization: Bearer ${PLACEHOLDER}`]);
    assert((await newRowsFor(sidecar, gist, before)).some((r) => r.swapped), "the gist request's audit row lacks the swap marker");
  });

  it("6. lets mise resolve an aqua tool through the placeholder", () => {
    // The app exports GITHUB_TOKEN and MISE_GITHUB_TOKEN only to its own mise calls, so `podman exec`
    // never carries them; set them here the way the app does for a sidecar sandbox.
    const r = guestSh(A, `GITHUB_TOKEN=${PLACEHOLDER} MISE_GITHUB_TOKEN=${PLACEHOLDER} mise ls-remote ${AQUA_TOOL}`);
    assert.equal(r.status, 0, `mise ls-remote failed: ${r.stderr}`);
    assert(!/\b(401|404)\b/.test(r.stderr), `mise reported an HTTP error: ${r.stderr}`);
    assert(r.stdout.length > 0, "mise ls-remote listed no versions");
  });

  it("7. refreshes the sidecar on rotation, stopped save and delete", async () => {
    // Rotation: a new stamp, and the guest keeps the same placeholder.
    await invokeCmd("set_github_token", { token: TOKEN_ALT });
    await waitForTokenFile(TOKEN_ALT);
    await waitForCardState("applied");
    assert(hostsYmlHolds(A, PLACEHOLDER), "hosts.yml must still hold the placeholder after rotation");
    await waitForGhApiUser(A);

    // A stopped sidecar's files are rewritten offline.
    await invokeCmd("stop_sandbox", { name: A });
    await browser.waitUntil(() => podman(LOCAL_CONNECTION, ["inspect", sidecar, "--format", "{{.State.Running}}"]) === "false", {
      timeout: SUBPROCESS,
      timeoutMsg: "the sidecar kept running after the sandbox stopped",
    });
    await invokeCmd("delete_credential_bundle", { serviceId: "github" });
    await waitForTokenFile(null);
    await invokeCmd("set_github_token", { token: TOKEN });
    await waitForTokenFile(TOKEN);
    await invokeCmd("start_sandbox", { name: A });
    await waitForGhApiUser(A);

    // Delete with the sandbox running drops the swap and the login.
    await invokeCmd("delete_credential_bundle", { serviceId: "github" });
    await waitForTokenFile(null);
    await browser.waitUntil(async () => ghApiUser(A).status !== 0, { timeout: SUBPROCESS, timeoutMsg: "gh api user still works after the credential was deleted" });
    assert(!hostsYmlHolds(A, TOKEN), "hosts.yml must not hold the token after delete");
    // The placeholder alone is rejected once the binding is gone, which shows the swap stopped.
    const direct = guestCurl(A, "https://api.github.com/user", [`Authorization: Bearer ${PLACEHOLDER}`]);
    assert.equal(direct.code, "401", `the placeholder must be rejected without a binding (got ${direct.code})`);
  });

  it("8a. opts A out: no token file, no login", async () => {
    await invokeCmd("set_github_token", { token: TOKEN });
    await waitForTokenFile(TOKEN);
    await waitForApplied(A);
    await invokeCmd("set_github_sandbox_access", { name: A, enabled: false });
    await waitForCardState("opted-out");
    await waitForTokenFile(null);
    await browser.waitUntil(async () => ghApiUser(A).status !== 0, { timeout: SUBPROCESS, timeoutMsg: "gh api user still works after opt-out" });
    assert(!hostsYmlHolds(A, TOKEN), "hosts.yml must not hold the token after opt-out");
  });
});
