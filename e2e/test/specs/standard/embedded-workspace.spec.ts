import assert from "node:assert";
import fs from "fs";
import path from "path";

import ProjectsPage from "../../pageobjects/projects.page.js";
import ProjectRow from "../../pageobjects/project-row.component.js";
import { podman, podmanOk, LOCAL_CONNECTION } from "../../../lib/podman-connection.js";
import { forwardList } from "../../../lib/mutagen.js";
import {
  workspaceState,
  setPanelCollapsed,
  switchApp,
  switchPane,
  paneFor,
  panesFor,
  openTerminalPane,
  closePane,
  type WorkspaceState,
} from "../../../lib/workspace.js";

// Logical-px constants mirrored from src-tauri/src/workspace/mod.rs. Asserted directly (not
// re-derived) so a spec failure points at either side changing without the other.
const GLOBAL_EXPANDED_W = 380;
const GLOBAL_RAIL_W = 48;
const SWITCHER_H = 64;

const WORKSPACE_ROOT = process.env.SANDBOX_WORKSPACE_ROOT;

// Poll for the host-side token file open_terminal_pane's shared prepare_terminal_webview helper
// writes (docs/techdebt.md workaround), proving the real zellij daemonize + token mint ran.
async function waitForToken(name: string): Promise<void> {
  const tokenPath = path.join(WORKSPACE_ROOT!, name, ".mio", "zellij.json");
  await browser.waitUntil(() => fs.existsSync(tokenPath), {
    timeout: 60000,
    interval: 1000,
    timeoutMsg: `token file ${tokenPath} did not appear after open_terminal_pane`,
  });
}

// `function ()` (not an arrow) so `this.timeout` raises the suite ceiling: the first create on a
// connection may trigger the one-time mise base-image build (~324 MB).
describe("Embedded workspace (multi-webview container)", function () {
  this.timeout(600000);

  const nameA = `e2e-workspace-a-${Date.now()}`;
  const nameB = `e2e-workspace-b-${Date.now()}`;
  const rowA: ProjectRow = ProjectsPage.row(nameA);
  const rowB: ProjectRow = ProjectsPage.row(nameB);
  let workspaceDirA: string;

  before(() => {
    expect(WORKSPACE_ROOT).toBeTruthy();
    workspaceDirA = path.join(WORKSPACE_ROOT!, nameA);
  });

  after(() => {
    // Best-effort cleanup so a failed run leaves no container behind.
    podman(LOCAL_CONNECTION, ["rm", "-f", nameA], { check: false });
    podman(LOCAL_CONNECTION, ["rm", "-f", nameB], { check: false });
  });

  // Capture the webview state on any failure (URL + page source + screenshot to e2e/.diag).
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

  it("creates and starts sandbox A", async () => {
    await ProjectsPage.open();
    // The wide wait covers a possible one-time base-image build.
    await ProjectsPage.createProject(nameA, { timeout: 600000 });
    await rowA.start();
    await rowA.waitForRunning();
  });

  it("opening the workspace embeds and activates only the Manage/Dashboard pane (Terminal is not started)", async () => {
    await rowA.openWorkspace();

    await browser.waitUntil(async () => panesFor(await workspaceState(), nameA).length === 1, {
      timeout: 30000,
      interval: 500,
      timeoutMsg: `sandbox "${nameA}" never registered its Dashboard pane after openWorkspace`,
    });

    const state = await workspaceState();
    const panes = panesFor(state, nameA);
    assert.strictEqual(
      panes.length,
      1,
      `Expected exactly one pane (Dashboard) for "${nameA}" right after openWorkspace, got ${panes.length}: ` +
        `${panes.map((p) => p.kind).join(", ")}. This can happen when open_workspace still calls embed_terminal.`,
    );

    const dashboard = paneFor(state, nameA, "dashboard")!;
    assert.strictEqual(dashboard.kind, "dashboard");
    assert(
      dashboard.url.includes("/workspace/dashboard"),
      `Dashboard pane url "${dashboard.url}" is not the internal /workspace/dashboard route.`,
    );
    assert.strictEqual(
      dashboard.forward_name,
      "",
      "The Dashboard pane has no Mutagen forward (it is pure Tauri IPC); forward_name must stay empty.",
    );
    assert.strictEqual(
      state.active_app,
      dashboard.label,
      "The Dashboard pane must be active immediately on creation — open_workspace no longer embeds " +
        "Terminal first, so embed_dashboard must activate the pane it creates instead of hiding it.",
    );
    assert(
      !fs.existsSync(path.join(workspaceDirA, ".mio", "zellij.json")),
      "zellij.json exists before any Terminal pane was requested — prepare_terminal_webview ran " +
        "eagerly during open_workspace, defeating the lazy-Terminal goal.",
    );

    // Structural layout proof (mirrors the Rust unit tests in workspace/mod.rs): the global panel
    // is fixed-width regardless of window size, and the switcher/content regions start exactly
    // where it ends — the same non-overlap invariant `workspace_layout` guarantees. SWITCHER_H is
    // 64 (grown from 40) to fit the two-level switcher's name row + tab row.
    assert.strictEqual(state.rects.global.width, GLOBAL_EXPANDED_W);
    assert.strictEqual(state.rects.switcher.height, SWITCHER_H);
    assert.strictEqual(state.rects.switcher.x, GLOBAL_EXPANDED_W);
    assert.strictEqual(state.rects.content.x, GLOBAL_EXPANDED_W);
  });

  it("open_terminal_pane lazily embeds the Terminal pane and makes it active", async () => {
    await openTerminalPane(nameA, workspaceDirA);
    await waitForToken(nameA);

    const state = await workspaceState();
    const panes = panesFor(state, nameA);
    assert.strictEqual(
      panes.length,
      2,
      `Expected exactly two panes (Dashboard + Terminal) for "${nameA}" after open_terminal_pane, got ${panes.length}.`,
    );

    const pane = paneFor(state, nameA, "terminal")!;
    assert.strictEqual(pane.kind, "terminal");
    assert(
      pane.url.startsWith("http://127.0.0.1:"),
      `Pane url "${pane.url}" is not a loopback forward — embed_terminal must use the ` +
        "Mutagen-forwarded Windows-host url, not the container-side address.",
    );
    // Phase C: the URL path pins a stable per-sandbox session name (the sandbox name itself) so a
    // later close+reopen reattaches to the same Zellij session instead of spawning a new one.
    assert.strictEqual(
      new URL(pane.url).pathname,
      `/${nameA}`,
      `Terminal pane url "${pane.url}" does not carry "/${nameA}" as its session path — ` +
        "prepare_terminal_webview must build the url as http://127.0.0.1:<port>/<sandbox-name>.",
    );
    assert.strictEqual(
      state.active_app,
      pane.label,
      "The freshly embedded Terminal pane must become the active tab.",
    );
  });

  it("switch_pane brings the Dashboard pane forward and records it as the sandbox's last-active pane", async () => {
    const terminal = paneFor(await workspaceState(), nameA, "terminal")!;
    const dashboard = paneFor(await workspaceState(), nameA, "dashboard")!;

    await switchPane(nameA, "dashboard");
    const afterToDashboard = await workspaceState();
    assert.strictEqual(afterToDashboard.active_app, dashboard.label);
    assert.strictEqual(
      afterToDashboard.sandbox_last_pane[nameA],
      dashboard.label,
      "switch_pane must record the sandbox's last-active pane so a later switch_app restores it.",
    );

    // The projects-list row narrows once nameA's workspace is open (Track 1 + Resolved Decision 3,
    // docs/features/responsive-panel/02_dev.md): the inline Package/Agent controls are gone, and
    // the remaining buttons drop their text label at the panel's real 380px width
    // (GLOBAL_EXPANDED_W, asserted above) — not a resized/simulated window.
    assert.strictEqual(
      await (await rowA.root.$("input[placeholder='package name']")).isExisting(),
      false,
      "Inline package management must never render in the projects-list row (it moved to the dashboard pane).",
    );
    assert.strictEqual(
      await (await rowA.root.$("button=Add agent")).isExisting(),
      false,
      "Inline agent management must never render in the projects-list row (it moved to the dashboard pane).",
    );
    const workspaceLabel = await (await rowA.workspaceButton).$("span");
    assert.strictEqual(
      await workspaceLabel.isDisplayed(),
      false,
      "The workspace button's text label must be hidden at the panel's real 380px width — the " +
        "max-[420px]: breakpoint did not fire, or main-global's viewport is not actually 380px.",
    );

    // Leave nameA back on its Terminal pane so the pre-existing switch_app assertion below (which
    // predates the two-pane model) keeps its original meaning unchanged.
    await switchPane(nameA, "terminal");
    assert.strictEqual((await workspaceState()).active_app, terminal.label);
  });

  it("the embedded terminal actually serves (server online, forward connected, HTTP 200)", async () => {
    const status = podman(LOCAL_CONNECTION, ["exec", nameA, "zellij", "web", "--status"], {
      check: false,
    });
    assert(status.includes("online"), `zellij web --status reported "${status}", expected "online".`);

    const fwdOut = forwardList(`terminal-${nameA}`);
    assert(
      fwdOut.includes("Connected"),
      `mutagen forward list terminal-${nameA} did not show Connected. Output:\n${fwdOut}`,
    );

    const state = await workspaceState();
    const pane = paneFor(state, nameA, "terminal")!;
    let status200 = false;
    await browser.waitUntil(
      async () => {
        try {
          status200 = (await fetch(pane.url)).status === 200;
          return status200;
        } catch {
          return false;
        }
      },
      { timeout: 30000, interval: 1000, timeoutMsg: `${pane.url} never returned HTTP 200 from the host.` },
    );
    assert(status200, `${pane.url} did not serve HTTP 200`);
  });

  it("collapsing and expanding the global panel snaps the layout to the rail and back", async () => {
    const before = await workspaceState();
    const beforeContentX = before.rects.content.x;

    await setPanelCollapsed(true);
    const collapsed = await workspaceState();
    assert.strictEqual(collapsed.collapsed, true);
    assert.strictEqual(collapsed.rects.global.width, GLOBAL_RAIL_W);
    assert.strictEqual(collapsed.rects.content.x, GLOBAL_RAIL_W);
    assert(
      collapsed.rects.content.width > before.rects.content.width,
      "Collapsing the rail must grow the content area.",
    );

    await setPanelCollapsed(false);
    const expanded = await workspaceState();
    assert.strictEqual(expanded.collapsed, false);
    assert.strictEqual(expanded.rects.global.width, GLOBAL_EXPANDED_W);
    assert.strictEqual(expanded.rects.content.x, beforeContentX);
  });

  it("close_pane closes only the Terminal tab, leaving Dashboard open and active", async () => {
    const before = await workspaceState();
    const terminalBefore = paneFor(before, nameA, "terminal")!;
    const dashboard = paneFor(before, nameA, "dashboard")!;

    await closePane(nameA, "terminal");

    await browser.waitUntil(
      async () => !forwardList(`terminal-${nameA}`).includes("Connected"),
      {
        timeout: 30000,
        interval: 1000,
        timeoutMsg: `terminal-${nameA} forward still shows Connected after close_pane.`,
      },
    );

    const state = await workspaceState();
    const panes = panesFor(state, nameA);
    assert.strictEqual(
      panes.length,
      1,
      `Expected only the Dashboard pane for "${nameA}" after closing Terminal, got ${panes.length}: ` +
        `${panes.map((p) => p.kind).join(", ")}.`,
    );
    assert.strictEqual(panes[0].kind, "dashboard");
    assert.strictEqual(
      state.active_app,
      dashboard.label,
      "close_pane must re-activate the sandbox's Dashboard pane once its Terminal pane closes.",
    );
    // The sandbox is not empty (Dashboard remains), so the global panel must stay narrow, not
    // return to full-bleed — that is close_workspace's contract, not close_pane's.
    assert.strictEqual(state.rects.global.width, GLOBAL_EXPANDED_W);

    // close_pane must never accept AppKind::Dashboard (Resolved Decision 2): a no-op, not a crash.
    await closePane(nameA, "dashboard");
    const afterNoop = await workspaceState();
    assert.strictEqual(
      panesFor(afterNoop, nameA).length,
      1,
      "close_pane(kind: dashboard) must be a no-op — the Manage/Home tab is not closable.",
    );

    // Reopening reattaches to the same Zellij session: the URL's stable session-name path
    // (Phase C) is unchanged across close + reopen.
    await openTerminalPane(nameA, workspaceDirA);
    const reopened = paneFor(await workspaceState(), nameA, "terminal")!;
    assert.strictEqual(
      new URL(reopened.url).pathname,
      new URL(terminalBefore.url).pathname,
      "Reopening the Terminal pane must reuse the same session-name URL path as before closing it.",
    );
  });

  it("re-opening an already-embedded sandbox is idempotent (switches, does not re-embed)", async () => {
    await rowA.openWorkspace();
    const state = await workspaceState();
    assert.strictEqual(
      panesFor(state, nameA).length,
      2,
      "Re-opening must not push additional panes for the same sandbox (still exactly Dashboard + Terminal).",
    );
    const fwdOut = forwardList(`terminal-${nameA}`);
    assert(
      (fwdOut.match(/^Status: Forwarding$/gm) ?? []).length === 1,
      `Expected exactly one Connected terminal-${nameA} forward after re-open. Output:\n${fwdOut}`,
    );
  });

  it("a second sandbox's workspace coexists with the first", async () => {
    await ProjectsPage.open();
    await ProjectsPage.createProject(nameB, { timeout: 600000 });
    await rowB.start();
    await rowB.waitForRunning();
    await rowB.openWorkspace();

    const state = await workspaceState();
    const paneA = paneFor(state, nameA, "terminal");
    const paneB = paneFor(state, nameB, "dashboard");
    assert(paneA, `sandbox A's terminal pane disappeared once B's workspace opened.`);
    assert(paneB, `workspace_state() has no dashboard pane for "${nameB}".`);
    assert.strictEqual(
      state.active_app,
      paneB!.label,
      "Opening B's workspace must make its Dashboard pane the active tab, leaving A embedded but hidden.",
    );
    assert(
      forwardList(`terminal-${nameA}`).includes("Connected"),
      "Sandbox A's forward must stay connected while B's workspace opens.",
    );
  });

  it("switch_app brings a hidden pane back to the front", async () => {
    const before = await workspaceState();
    const paneA = paneFor(before, nameA, "terminal")!;

    await switchApp(nameA);
    const after = await workspaceState();
    assert.strictEqual(after.active_app, paneA.label);
  });

  it("destroying one sandbox tears down every one of its panes and its forward", async () => {
    const beforeState = await workspaceState();
    const paneB = paneFor(beforeState, nameB, "dashboard")!;

    await rowA.destroy();

    assert(!podmanOk(LOCAL_CONNECTION, ["inspect", nameA]), `Local container "${nameA}" still exists after destroy.`);

    const afterState = await workspaceState();
    assert.strictEqual(
      panesFor(afterState, nameA).length,
      0,
      `workspace_state() still reports a pane for "${nameA}" after destroy — destroy_sandbox must call ` +
        "workspace::close_app for every one of the sandbox's panes.",
    );
    assert.strictEqual(
      afterState.sandbox_last_pane[nameA],
      undefined,
      `sandbox_last_pane still has an entry for "${nameA}" after destroy — close_app must clear it.`,
    );
    assert(
      !forwardList(`terminal-${nameA}`).includes("Connected"),
      `terminal-${nameA} forward still shows Connected after destroy.`,
    );

    // Sandbox B's workspace must be untouched: same pane, no forward to check (Dashboard has none).
    const paneBAfter = paneFor(afterState, nameB, "dashboard");
    assert(paneBAfter, "Sandbox B's pane disappeared after sandbox A was destroyed.");
    assert.strictEqual(paneBAfter!.label, paneB.label);
  });

  it("destroying the last sandbox returns the global panel to full-bleed", async () => {
    await rowB.destroy();

    assert(!podmanOk(LOCAL_CONNECTION, ["inspect", nameB]), `Local container "${nameB}" still exists after destroy.`);

    const state: WorkspaceState = await workspaceState();
    assert.strictEqual(state.apps.length, 0);
    assert.strictEqual(state.active_app, null);
    // No content: workspace_layout's !has_content branch collapses switcher/content to zero-width
    // and the global panel fills the window (see workspace/mod.rs's without_content unit test).
    assert.strictEqual(state.rects.switcher.width, 0);
    assert.strictEqual(state.rects.content.width, 0);
  });
});
