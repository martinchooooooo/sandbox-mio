import assert from "node:assert";
import path from "path";

import ProjectsPage from "../../pageobjects/projects.page.js";
import ProjectRow from "../../pageobjects/project-row.component.js";
import workspaceDashboard from "../../pageobjects/workspace-dashboard.page.js";
import sandboxHeader from "../../pageobjects/sandbox-header.page.js";
import sidebar from "../../pageobjects/sidebar.page.js";
import { podman, podmanOk, LOCAL_CONNECTION } from "../../../lib/podman-connection.js";
import { forwardList } from "../../../lib/mutagen.js";
import { captureDiagOnFailure } from "../../../lib/diag.js";
import {
  workspaceState,
  setPanelCollapsed,
  switchApp,
  hideActivePane,
  paneFor,
  panesFor,
  openTerminalPane,
  closePane,
  type WorkspaceState,
} from "../../../lib/workspace.js";

// Logical-px constants mirrored from src-tauri/src/workspace/mod.rs (the app sidebar's expanded and
// rail widths, the sandbox header's height). Asserted directly (not re-derived) so a spec failure
// points at either side changing without the other.
const NAVIGATION_PANEL_EXPANDED_W = 240;
const NAVIGATION_PANEL_RAIL_W = 48;
const SWITCHER_H = 64;

const WORKSPACE_ROOT = process.env.SANDBOX_WORKSPACE_ROOT;

// Zellij's own view of the sandbox: whether the web server runs and whether a login token exists.
// `--status` exits non-zero when offline, hence `check: false`; "online" is not a substring of
// "offline". Token lines read `token_<n>: ...`, unlike the "no tokens" notice.
function zellijWebState(name: string): { online: boolean; tokens: boolean } {
  const status = podman(LOCAL_CONNECTION, ["exec", name, "zellij", "web", "--status"], { check: false });
  const list = podman(LOCAL_CONNECTION, ["exec", name, "zellij", "web", "--list-tokens"], { check: false });
  return { online: status.includes("online"), tokens: /^token_\d+/m.test(list) };
}

// Proves prepare_terminal's daemonize + token mint ran, without any token file in the workspace.
async function waitForZellijLogin(name: string): Promise<void> {
  await browser.waitUntil(
    () => {
      const state = zellijWebState(name);
      return state.online && state.tokens;
    },
    {
      timeout: 60000,
      interval: 1000,
      timeoutMsg: `zellij web in "${name}" never reported online with a minted token after open_terminal_pane`,
    },
  );
}

// Dashboard is DOM-derived client state, not a Rust-tracked pane (docs/features/consolidate-app-
// container/02_dev.md Resolved Decision 2), so only reading the rendered DOM — not workspace_state()
// — can catch a present-but-zero-height regression (the old switcher's h-screen/h-16 bug).
async function assertDashboardVisible(sandbox: string): Promise<void> {
  await workspaceDashboard.waitForRendered(sandbox);
  await workspaceDashboard.openSection("tools");
  const input = await workspaceDashboard.packageInput;
  assert(
    await input.isDisplayed(),
    `Dashboard content for "${sandbox}" exists in the DOM but is not displayed.`,
  );
  const { height } = await input.getSize();
  assert(
    height > 0,
    `Dashboard content for "${sandbox}" has zero rendered height — this is the switcher/header ` +
      "h-screen/h-16 class of bug (docs/features/consolidate-app-container/02_dev.md Common Gotchas).",
  );
}

// `function ()` (not an arrow) so `this.timeout` raises the suite ceiling: the first create on a
// connection may trigger the one-time mise base-image build (~324 MB).
describe("Embedded workspace (app sidebar, sandbox header and Dashboard in one webview)", function () {
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
  captureDiagOnFailure();

  it("creates and starts sandbox A", async () => {
    await ProjectsPage.open();
    // The wide wait covers a possible one-time base-image build.
    await ProjectsPage.createProject(nameA, { timeout: 600000 });
    await rowA.start();
    await rowA.waitForRunning();
  });

  it("opening the workspace shows sandbox A's Dashboard with no pane registered (Terminal is not started)", async () => {
    await rowA.openWorkspace();
    await assertDashboardVisible(nameA);

    const state = await workspaceState();
    assert.strictEqual(
      panesFor(state, nameA).length,
      0,
      "No AppPane should exist for a sandbox that has only ever been viewed via Dashboard " +
        "(Resolved Decision 2) — Dashboard is client-side view state, not a Rust-tracked pane.",
    );
    assert.strictEqual(
      state.active_app,
      null,
      "active_app must stay null while the client-side view is Dashboard for any sandbox.",
    );
    const zellij = zellijWebState(nameA);
    assert(
      !zellij.online && !zellij.tokens,
      `zellij web in "${nameA}" is online or holds a token before any Terminal pane was requested ` +
        `(${JSON.stringify(zellij)}) — prepare_terminal ran eagerly during watch_workspace_ports, ` +
        "defeating the lazy-Terminal goal.",
    );

    // Structural layout proof (mirrors the Rust unit tests in workspace/mod.rs): the overlay rect is
    // always positioned at the sidebar's edge and below the SWITCHER_H-tall header, regardless of active pane —
    // the merged webview itself is never resized.
    assert.strictEqual(state.overlay.x, NAVIGATION_PANEL_EXPANDED_W);
    assert.strictEqual(state.overlay.y, SWITCHER_H);
  });

  it("open_terminal_pane lazily embeds the Terminal pane and makes it active", async () => {
    await openTerminalPane(nameA, workspaceDirA);
    await waitForZellijLogin(nameA);

    const state = await workspaceState();
    const panes = panesFor(state, nameA);
    assert.strictEqual(
      panes.length,
      1,
      `Expected exactly one pane (Terminal) for "${nameA}" after open_terminal_pane, got ${panes.length}.`,
    );

    const pane = paneFor(state, nameA, "terminal")!;
    assert.strictEqual(pane.kind, "terminal");
    assert(
      pane.url!.startsWith("http://127.0.0.1:"),
      `Pane url "${pane.url}" is not a loopback forward — embed_terminal must use the ` +
        "Mutagen-forwarded Windows-host url, not the container-side address.",
    );
    // Phase C: the URL path pins a stable per-sandbox session name (the sandbox name itself) so a
    // later close+reopen reattaches to the same Zellij session instead of spawning a new one.
    assert.strictEqual(
      new URL(pane.url!).pathname,
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

  it("clicking the sandbox header's Manage tab hides the Terminal overlay and shows Dashboard again", async () => {
    const terminal = paneFor(await workspaceState(), nameA, "terminal")!;

    await sandboxHeader.clickTab(nameA, "dashboard");
    await assertDashboardVisible(nameA);

    const afterToDashboard = await workspaceState();
    assert.strictEqual(
      afterToDashboard.active_app,
      null,
      "hide_active_pane must clear active_app once the Manage tab is clicked.",
    );
    assert.strictEqual(
      afterToDashboard.sandbox_last_pane[nameA],
      terminal.label,
      "sandbox_last_pane must still remember the Terminal pane so a later switch_app restores it, " +
        "even though it is no longer the active view.",
    );

    // Home is unmounted while a sandbox is in view, so no card can shadow the header's
    // `data-sandbox` (docs/features/unified-sidebar-nav/02_dev.md, Resolved Decision 9).
    assert.strictEqual(
      await (await rowA.root).isExisting(),
      false,
      "Sandbox A's Home card is still mounted while A is in view — the routed page must unmount.",
    );

    // Bring the Terminal pane back so the switch_app assertion below keeps its meaning.
    await sandboxHeader.clickTab(nameA, "terminal");
    await waitForZellijLogin(nameA);
    assert.strictEqual((await workspaceState()).active_app, terminal.label);
  });

  it("showing Home hides the Terminal pane without closing it, and the sidebar restores it", async () => {
    const terminal = paneFor(await workspaceState(), nameA, "terminal")!;

    await ProjectsPage.open();
    const onHome = await workspaceState();
    assert.strictEqual(onHome.active_app, null, "Showing Home must hide the active pane (hide_active_pane).");
    assert(paneFor(onHome, nameA, "terminal"), "Showing Home closed sandbox A's Terminal pane instead of hiding it.");
    assert(
      forwardList(`terminal-${nameA}`).includes("Connected"),
      "Sandbox A's terminal forward must stay connected while Home shows.",
    );

    // Home cards never carry inline package or agent management (it lives in the Dashboard), and
    // their labels stay visible now that pages use the full content area.
    assert.strictEqual(
      await (await rowA.root.$("input[placeholder='package name']")).isExisting(),
      false,
      "Inline package management must never render in a Home card (it moved to the Dashboard region).",
    );
    assert.strictEqual(
      await (await rowA.root.$("button=Add agent")).isExisting(),
      false,
      "Inline agent management must never render in a Home card (it moved to the Dashboard region).",
    );
    assert(
      await (await (await rowA.workspaceButton).$("span")).isDisplayed(),
      "The Home card's workspace label is hidden — Home cards keep their labels at full width.",
    );

    // The last view was the Terminal pane, so the sidebar entry restores it rather than Manage.
    await sidebar.selectSandbox(nameA);
    await browser.waitUntil(async () => (await workspaceState()).active_app === terminal.label, {
      timeout: 10000,
      timeoutMsg: "Selecting sandbox A in the sidebar did not restore its Terminal pane.",
    });
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
          status200 = (await fetch(pane.url!)).status === 200;
          return status200;
        } catch {
          return false;
        }
      },
      { timeout: 30000, interval: 1000, timeoutMsg: `${pane.url} never returned HTTP 200 from the host.` },
    );
    assert(status200, `${pane.url} did not serve HTTP 200`);
  });

  it("collapsing and expanding the app sidebar moves the overlay rect to the rail and back", async () => {
    const before = await workspaceState();
    const beforeOverlayX = before.overlay.x;
    assert.strictEqual(beforeOverlayX, NAVIGATION_PANEL_EXPANDED_W);

    await setPanelCollapsed(true);
    const collapsed = await workspaceState();
    assert.strictEqual(collapsed.collapsed, true);
    assert.strictEqual(collapsed.overlay.x, NAVIGATION_PANEL_RAIL_W);
    assert(
      collapsed.overlay.width > before.overlay.width,
      "Collapsing the app sidebar must grow the overlay's available width.",
    );
    await browser.waitUntil(async () => sidebar.isCollapsed(), {
      timeout: 5000,
      timeoutMsg: "The app sidebar never rendered as the rail after set_panel_collapsed(true).",
    });

    await setPanelCollapsed(false);
    const expanded = await workspaceState();
    assert.strictEqual(expanded.collapsed, false);
    assert.strictEqual(expanded.overlay.x, beforeOverlayX);

    // The Terminal pane is still the active view throughout — collapsing/expanding re-bounds the
    // panes and restyles the sidebar; it must not disturb which pane is active.
    assert.strictEqual(expanded.active_app, paneFor(before, nameA, "terminal")!.label);
  });

  it("close_pane closes the Terminal tab, leaving the client-side view to fall back to Dashboard", async () => {
    const before = await workspaceState();
    const terminalBefore = paneFor(before, nameA, "terminal")!;

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
      0,
      `Expected no panes for "${nameA}" after closing its only Terminal pane, got ${panes.length}: ` +
        `${panes.map((p) => p.kind).join(", ")}.`,
    );
    assert.strictEqual(
      state.active_app,
      null,
      "close_pane must clear active_app once the closed pane was the active one, letting the " +
        "client-side view fall back to Dashboard (no Dashboard pane label to re-activate).",
    );
    await assertDashboardVisible(nameA);

    // Reopening reattaches to the same Zellij session: the URL's stable session-name path
    // (Phase C) is unchanged across close + reopen.
    await openTerminalPane(nameA, workspaceDirA);
    const reopened = paneFor(await workspaceState(), nameA, "terminal")!;
    assert.strictEqual(
      new URL(reopened.url!).pathname,
      new URL(terminalBefore.url!).pathname,
      "Reopening the Terminal pane must reuse the same session-name URL path as before closing it.",
    );
  });

  it("re-opening an already-embedded sandbox is idempotent (switches, does not re-embed)", async () => {
    await rowA.openWorkspace();
    const state = await workspaceState();
    assert.strictEqual(
      panesFor(state, nameA).length,
      1,
      "Re-opening must not push additional panes for the same sandbox (still exactly one Terminal pane).",
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
    assert(paneA, `sandbox A's terminal pane disappeared once B's workspace opened.`);
    assert.strictEqual(
      panesFor(state, nameB).length,
      0,
      `sandbox "${nameB}" should have no registered pane — it has only been viewed via Dashboard.`,
    );
    assert.strictEqual(
      state.active_app,
      null,
      "Opening B's workspace shows its Dashboard (client-side view state, no Rust pane), so " +
        "active_app must be null, leaving A's Terminal pane embedded but hidden underneath.",
    );
    await assertDashboardVisible(nameB);
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

    // Sandbox B's workspace must be untouched: it never had a Rust-tracked pane to begin with, so
    // there is nothing to assert on workspace_state() for it — its Dashboard is still reachable.
    await rowB.openWorkspace();
    await assertDashboardVisible(nameB);
  });

  it("destroying the last sandbox leaves the overlay rect at its expanded default", async () => {
    await rowB.destroy();

    assert(!podmanOk(LOCAL_CONNECTION, ["inspect", nameB]), `Local container "${nameB}" still exists after destroy.`);

    const state: WorkspaceState = await workspaceState();
    assert.strictEqual(state.apps.length, 0);
    assert.strictEqual(state.active_app, null);
    // Unlike the pre-consolidation three-rect model, the overlay rect is not conditioned on
    // whether any sandbox is open (Resolved Decision 3) — it always reflects the current
    // sidebar-width/header-height layout, since the merged webview is never resized.
    assert.strictEqual(state.overlay.x, NAVIGATION_PANEL_EXPANDED_W);
    assert.strictEqual(state.overlay.y, SWITCHER_H);
  });
});
