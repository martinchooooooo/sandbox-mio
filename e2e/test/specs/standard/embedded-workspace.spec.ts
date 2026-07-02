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
  paneFor,
  type WorkspaceState,
} from "../../../lib/workspace.js";

// Logical-px constants mirrored from src-tauri/src/workspace/mod.rs. Asserted directly (not
// re-derived) so a spec failure points at either side changing without the other.
const GLOBAL_EXPANDED_W = 380;
const GLOBAL_RAIL_W = 48;
const SWITCHER_H = 40;

const WORKSPACE_ROOT = process.env.SANDBOX_WORKSPACE_ROOT;

// Poll for the host-side token file open_workspace's shared prepare_terminal_webview helper writes
// (docs/techdebt.md workaround), proving the real zellij daemonize + token mint ran — the same
// proof web-terminal.spec.ts uses for the standalone open_terminal path.
async function waitForToken(name: string): Promise<void> {
  const tokenPath = path.join(WORKSPACE_ROOT!, name, ".mio", "zellij.json");
  await browser.waitUntil(() => fs.existsSync(tokenPath), {
    timeout: 60000,
    interval: 1000,
    timeoutMsg: `token file ${tokenPath} did not appear after openWorkspace`,
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

  before(() => {
    expect(WORKSPACE_ROOT).toBeTruthy();
  });

  after(() => {
    // Best-effort cleanup so a failed run leaves no container behind.
    podman(LOCAL_CONNECTION, ["rm", "-f", nameA], { check: false });
    podman(LOCAL_CONNECTION, ["rm", "-f", nameB], { check: false });
  });

  // Capture the webview state on any failure (URL + page source + screenshot to e2e/.diag),
  // mirroring web-terminal.spec.ts.
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

  it("opening the workspace embeds the terminal as the active pane", async () => {
    await rowA.openWorkspace();
    await waitForToken(nameA);

    const state = await workspaceState();
    const pane = paneFor(state, nameA);
    assert(
      pane,
      `workspace_state() has no pane for "${nameA}" after openWorkspace. ` +
        "This can happen when open_workspace did not call workspace::embed_terminal.",
    );
    assert.strictEqual(pane!.kind, "terminal");
    assert(
      pane!.url.startsWith("http://127.0.0.1:"),
      `Pane url "${pane!.url}" is not a loopback forward — embed_terminal must use the ` +
        "Mutagen-forwarded Windows-host url, not the container-side address.",
    );
    assert.strictEqual(
      state.active_app,
      pane!.label,
      "The freshly embedded pane must become the active tab.",
    );

    // Structural layout proof (mirrors the Rust unit tests in workspace/mod.rs): the global panel
    // is fixed-width regardless of window size, and the switcher/content regions start exactly
    // where it ends — the same non-overlap invariant `workspace_layout` guarantees.
    assert.strictEqual(state.rects.global.width, GLOBAL_EXPANDED_W);
    assert.strictEqual(state.rects.switcher.height, SWITCHER_H);
    assert.strictEqual(state.rects.switcher.x, GLOBAL_EXPANDED_W);
    assert.strictEqual(state.rects.content.x, GLOBAL_EXPANDED_W);
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
    const pane = paneFor(state, nameA)!;
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

  it("re-opening an already-embedded sandbox is idempotent (switches, does not re-embed)", async () => {
    await rowA.openWorkspace();
    const state = await workspaceState();
    assert.strictEqual(
      state.apps.filter((p) => p.sandbox === nameA).length,
      1,
      "Re-opening must not push a second pane for the same sandbox.",
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
    await waitForToken(nameB);

    const state = await workspaceState();
    const paneA = paneFor(state, nameA);
    const paneB = paneFor(state, nameB);
    assert(paneA, `sandbox A's pane disappeared once B's workspace opened.`);
    assert(paneB, `workspace_state() has no pane for "${nameB}".`);
    assert.strictEqual(
      state.active_app,
      paneB!.label,
      "Opening B's workspace must make B the active tab, leaving A embedded but hidden.",
    );
    assert(
      forwardList(`terminal-${nameA}`).includes("Connected"),
      "Sandbox A's forward must stay connected while B's workspace opens.",
    );
    assert(
      forwardList(`terminal-${nameB}`).includes("Connected"),
      `mutagen forward list terminal-${nameB} did not show Connected.`,
    );
  });

  it("switch_app brings a hidden pane back to the front", async () => {
    const before = await workspaceState();
    const paneA = paneFor(before, nameA)!;

    await switchApp(nameA);
    const after = await workspaceState();
    assert.strictEqual(after.active_app, paneA.label);
  });

  it("destroying one sandbox tears down only its own pane and forward", async () => {
    const beforeState = await workspaceState();
    const paneB = paneFor(beforeState, nameB)!;

    await rowA.destroy();

    assert(!podmanOk(LOCAL_CONNECTION, ["inspect", nameA]), `Local container "${nameA}" still exists after destroy.`);

    const afterState = await workspaceState();
    assert(
      !paneFor(afterState, nameA),
      `workspace_state() still reports a pane for "${nameA}" after destroy — destroy_sandbox must call ` +
        "workspace::close_app.",
    );
    assert(
      !forwardList(`terminal-${nameA}`).includes("Connected"),
      `terminal-${nameA} forward still shows Connected after destroy.`,
    );

    // Sandbox B's workspace must be untouched: same pane, forward still live.
    const paneBAfter = paneFor(afterState, nameB);
    assert(paneBAfter, "Sandbox B's pane disappeared after sandbox A was destroyed.");
    assert.strictEqual(paneBAfter!.label, paneB.label);
    assert(
      forwardList(`terminal-${nameB}`).includes("Connected"),
      "Destroying sandbox A must not touch sandbox B's forward.",
    );
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
    assert(
      !forwardList(`terminal-${nameB}`).includes("Connected"),
      `terminal-${nameB} forward still shows Connected after destroy.`,
    );
  });
});
