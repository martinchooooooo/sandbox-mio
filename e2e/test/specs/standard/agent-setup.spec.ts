import assert from "node:assert";
import fs from "fs";
import path from "path";

import ProjectsPage from "../../pageobjects/projects.page.js";
import ProjectRow from "../../pageobjects/project-row.component.js";
import workspaceDashboard from "../../pageobjects/workspace-dashboard.page.js";
import { podman, podmanOk, LOCAL_CONNECTION } from "../../../lib/podman-connection.js";

const WORKSPACE_ROOT = process.env.SANDBOX_WORKSPACE_ROOT;

// `browser.url` rejects relative paths, and there is no wdio `baseUrl`: the app is served from a
// Tauri custom-protocol origin (e.g. http://tauri.localhost). Resolve "/" against the live origin
// so the navigation is a full reload to home, which remounts the projects page (onMount →
// loadProjects → loadAgentStatuses).
async function navigateHome(): Promise<void> {
  const origin = new URL(await browser.getUrl()).origin;
  await browser.url(`${origin}/`);
}

// `function ()` (not an arrow) so `this.timeout` raises the suite ceiling: node@lts downloads
// ~30 MB and the claude-code npm package + native postinstall binary add another ~40 MB.
describe("Agent setup — Claude Code", function () {
  this.timeout(900000);

  const projectName = `e2e-agents-${Date.now()}`;
  const row: ProjectRow = ProjectsPage.row(projectName);
  let workspaceDir: string;
  let manifestPath: string;

  before(() => {
    expect(WORKSPACE_ROOT).toBeTruthy();
    workspaceDir = path.join(WORKSPACE_ROOT!, projectName);
    manifestPath = path.join(workspaceDir, "mise.toml");
  });

  after(() => {
    podman(LOCAL_CONNECTION, ["rm", "-f", projectName], { check: false });
  });

  // Safety net: if a test above throws while still attached to the dashboard webview (an
  // assertion failure between switchToDashboard and switchToAppWebview), later tests must not
  // inherit that attachment — the global `before` hook only pins the session once per session.
  afterEach(async () => {
    await workspaceDashboard.switchToAppWebview().catch(() => {});
  });

  afterEach(async function () {
    if (this.currentTest?.state !== "failed") return;
    const diag = path.join(process.cwd(), ".diag");
    fs.mkdirSync(diag, { recursive: true });
    const stamp = (this.currentTest?.title ?? "test").replace(/[^a-z0-9]+/gi, "-").slice(0, 40);
    try {
      const url = await browser.getUrl();
      const src = await browser.getPageSource();
      fs.writeFileSync(path.join(diag, `${stamp}.html`), `<!-- url: ${url} -->\n${src}`);
      await browser.saveScreenshot(path.join(diag, `${stamp}.png`));
      console.log(`[diag] url=${url} — dumped ${stamp}.html/.png to ${diag}`);
    } catch (e) {
      console.log(`[diag] could not capture page state: ${String(e)}`);
    }
  });

  it("creates a project, opens its workspace, and shows an Agents section with no agents in the dashboard pane", async () => {
    await ProjectsPage.open();
    await ProjectsPage.createProject(projectName, { timeout: 600000 });

    await browser.waitUntil(() => fs.existsSync(manifestPath), {
      timeout: 60000,
      interval: 1000,
      timeoutMsg: `mise manifest ${manifestPath} did not appear after create`,
    });

    // Package/agent management now requires a workspace to be open (Resolved Decision 3,
    // docs/features/responsive-panel/02_dev.md) — there is no inline fallback in the row anymore.
    await row.openWorkspace();
    await workspaceDashboard.switchToDashboard(projectName);

    await workspaceDashboard.noAgentsText.waitForExist({
      timeout: 5000,
      timeoutMsg: "Agents section with 'No agents installed.' did not appear in the dashboard pane",
    });

    await workspaceDashboard.switchToAppWebview();
  });

  it("installs Claude Code through the UI including the npm postinstall step", async () => {
    await workspaceDashboard.switchToDashboard(projectName);
    await workspaceDashboard.startAgentInstall("Claude Code");

    // Wait for the agent row to appear — install downloads node@lts + claude-code + native binary.
    await workspaceDashboard.waitForAgentRow("Claude Code", 900000);

    const manifest = fs.readFileSync(manifestPath, "utf8");
    assert(
      manifest.includes("node"),
      `mise.toml does not list "node" after agent install. Contents:\n${manifest}`
    );
    assert(
      manifest.includes("npm:@anthropic-ai/claude-code"),
      `mise.toml does not list claude-code after agent install. Contents:\n${manifest}`
    );

    // Decisive assertion: `claude --version` proves the npm postinstall step ran. Without it
    // mise's --ignore-scripts=true leaves the native binary absent and this command fails.
    const ver = podman(LOCAL_CONNECTION, [
      "exec", "--workdir", "/root/work", projectName,
      "mise", "exec", "--", "claude", "--version",
    ]);
    assert(
      ver.toLowerCase().includes("claude"),
      `"claude --version" printed "${ver}". ` +
        "This can happen when the npm-postinstall step was skipped, leaving the native binary absent."
    );

    await workspaceDashboard.waitForAgentStatus("Claude Code", "not authenticated");
    await workspaceDashboard.switchToAppWebview();
  });

  it("shows authenticated status when credential files are pre-seeded", async () => {
    const expiresAt = Date.now() + 365 * 24 * 60 * 60 * 1000;
    podman(LOCAL_CONNECTION, [
      "exec", projectName, "bash", "-c",
      `mkdir -p /root/.claude && ` +
      `printf '%s' '{"claudeAiOauth":{"accessToken":"test","refreshToken":"test","expiresAt":${expiresAt}}}' ` +
      `> /root/.claude/.credentials.json && ` +
      `printf '%s' '{"oauthAccount":{"emailAddress":"e2e-test@example.com"}}' ` +
      `> /root/.claude.json`,
    ]);

    // Navigate away and back to trigger onMount → loadAgentStatuses. The reload only remounts
    // main-global; the workspace's panes (and the Rust registry backing them) are untouched.
    await navigateHome();
    await ProjectsPage.open();
    await row.waitForDisplayed();

    await row.openWorkspace();
    await workspaceDashboard.switchToDashboard(projectName);

    await workspaceDashboard.waitForAgentStatus("Claude Code", "e2e-test@example.com", 15000);

    const btn = await workspaceDashboard.agentAuthButton("Claude Code");
    await expect(btn).toHaveText(/re-authenticate/i);

    await workspaceDashboard.switchToAppWebview();
  });

  it("clicking Authenticate starts the auth process in the container", async () => {
    // Reset to unauthenticated state.
    podman(LOCAL_CONNECTION, ["exec", projectName, "bash", "-c",
      "rm -f /root/.claude/.credentials.json /root/.claude.json"]);

    await navigateHome();
    await ProjectsPage.open();
    await row.waitForDisplayed();
    await row.openWorkspace();
    await workspaceDashboard.switchToDashboard(projectName);
    await workspaceDashboard.waitForAgentStatus("Claude Code", "not authenticated");

    const authBtn = await workspaceDashboard.agentAuthButton("Claude Code");
    await authBtn.scrollIntoView();
    await authBtn.click();

    // Allow time for start_agent_auth → podman exec → mise exec → claude binary to start.
    await browser.pause(15000);

    // Scan /proc (no `ps` in the container) to confirm the process is running.
    const procs = podman(LOCAL_CONNECTION, [
      "exec", projectName, "bash", "-c",
      "for f in /proc/[0-9]*/cmdline; do tr '\\0' ' ' < \"$f\" 2>/dev/null; echo; done",
    ], { check: false });
    assert(
      procs.includes("claude") && procs.includes("auth") && procs.includes("login"),
      `No "claude auth login" process found in container after clicking Authenticate.\n` +
        `/proc scan:\n${procs}\n` +
        "This can happen when start_agent_auth did not spawn the process or the binary path is wrong."
    );

    await workspaceDashboard.switchToAppWebview();
  });

  it("destroy removes the container even with a running auth process", async () => {
    await row.destroy();
    assert(
      !podmanOk(LOCAL_CONNECTION, ["inspect", projectName]),
      `Container "${projectName}" still exists after destroy.`
    );
  });
});
