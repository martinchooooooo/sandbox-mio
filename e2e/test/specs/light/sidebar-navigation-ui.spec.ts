import assert from "node:assert";
import path from "node:path";

import ProjectsPage from "../../pageobjects/projects.page.js";
import ModelsPage from "../../pageobjects/models.page.js";
import SettingsPage from "../../pageobjects/settings.page.js";
import ActivityCenter from "../../pageobjects/activity.page.js";
import chat from "../../pageobjects/agent-chat.page.js";
import sidebar from "../../pageobjects/sidebar.page.js";
import sandboxHeader from "../../pageobjects/sandbox-header.page.js";
import workspaceDashboard from "../../pageobjects/workspace-dashboard.page.js";
import { isPageShown, navButton, showPage, type PageHref } from "../../pageobjects/app.page.js";
import { captureDiagOnFailure } from "../../../lib/diag.js";
import { invokeCmd } from "../../../lib/mock.js";
import { WORKSPACE_ROOT } from "../../../lib/run-identity.js";
import { COMMAND, PANEL_RENDER, UI_SETTLE } from "../../../lib/timeouts.js";
import { paneFor, panesFor, workspaceState } from "../../../lib/workspace.js";

// LIGHT spec for docs/features/unified-sidebar-nav (03_e2e.md, spec L1): the app sidebar, the
// content area's page-or-sandbox switch and the single sandbox header, against the mock backend.
// The native chat stands in for a pane: it is a real Rust-registered pane (active_app,
// sandbox_last_pane) that MockAgentHost serves without a webview or a forward. Pane geometry and
// webview panes are the standard tier's job (standard/embedded-workspace.spec.ts).

/** The sidebar page entries currently highlighted. */
async function activePageEntries(): Promise<string[]> {
  const entries: string[] = [];
  for (const el of await $$("button[data-nav][data-active='true']").getElements()) {
    entries.push((await el.getAttribute("data-nav")) ?? "");
  }
  return entries;
}

async function openChat(name: string): Promise<string> {
  await invokeCmd("open_agent_chat", { name, workspacePath: path.join(WORKSPACE_ROOT, name) });
  await (await chat.root(name)).waitForExist({ timeout: PANEL_RENDER });
  await chat.waitForConnection(name, "attached");
  return paneFor(await workspaceState(), name, "agentchat")!.label;
}

async function waitForActiveApp(label: string | null, why: string): Promise<void> {
  await browser.waitUntil(async () => (await workspaceState()).active_app === label, {
    timeout: COMMAND,
    timeoutMsg: why,
  });
}

describe("Unified sidebar navigation (mock backend)", function () {
  this.timeout(120000);

  captureDiagOnFailure();

  const nameA = `e2e-nav-a-${Date.now()}`;
  const nameB = `e2e-nav-b-${Date.now()}`;

  after(async () => {
    await invokeCmd("set_panel_collapsed", { collapsed: false }).catch(() => undefined);
    for (const name of [nameA, nameB]) {
      await invokeCmd("close_workspace", { name }).catch(() => undefined);
      await invokeCmd("destroy_sandbox", { name, force: true }).catch(() => undefined);
    }
  });

  it("moves between every page through the sidebar, highlighting exactly one entry", async () => {
    const pages: PageHref[] = ["/projects", "/models", "/credentials", "/settings", "/intro", "/projects"];
    for (const href of pages) {
      await showPage(href);
      assert.deepStrictEqual(
        await activePageEntries(),
        [href],
        `After choosing ${href} in the sidebar, the highlighted page entries are wrong.`,
      );
      assert.strictEqual(new URL(await browser.getUrl()).pathname, href);
      assert.strictEqual(await sandboxHeader.sandboxInView(), null, `A sandbox header shows on ${href}.`);
    }
    assert(await (await ProjectsPage.root).isDisplayed(), "Home (main[data-home]) is not showing after choosing Home.");
  });

  it("opens Manage from a Home card and unmounts Home while the sandbox is in view", async () => {
    await ProjectsPage.open();
    await ProjectsPage.ensurePodmanReady();
    await ProjectsPage.createProject(nameA);
    await ProjectsPage.createProject(nameB);

    await ProjectsPage.row(nameA).openWorkspace();
    await workspaceDashboard.waitForRendered(nameA);

    assert.strictEqual(await sandboxHeader.sandboxInView(), nameA);
    assert(await sandboxHeader.isTabActive(nameA, "dashboard"), "The header's Manage tab is not current.");
    assert.strictEqual(
      await (await ProjectsPage.root).isExisting(),
      false,
      "Home is still mounted while a sandbox is in view (docs/features/unified-sidebar-nav/02_dev.md, Resolved Decision 9).",
    );
    assert.strictEqual(await (await $("li[data-sandbox]")).isExisting(), false, "A Home card is still in the DOM.");
    // The URL stays on /projects, so only the sidebar can tell that no page is showing.
    assert.strictEqual(new URL(await browser.getUrl()).pathname, "/projects");
    assert.strictEqual(await isPageShown("/projects"), false, "The Home entry stays highlighted under a sandbox.");
    assert.deepStrictEqual(await activePageEntries(), []);
    assert(await sidebar.isSandboxActive(nameA), "The sandbox in view is not highlighted in the sidebar.");
    assert(await ActivityCenter.isToggleShown(), "The activity toggle is missing while a sandbox is in view.");
  });

  it("switches sandboxes in the sidebar, showing one header at a time", async () => {
    await sidebar.selectSandbox(nameB);
    await workspaceDashboard.waitForRendered(nameB);

    assert.strictEqual((await $$("div[data-sandbox-header]")).length, 1, "More than one sandbox header rendered.");
    assert.strictEqual(await sandboxHeader.sandboxInView(), nameB);
    assert(await sidebar.isSandboxActive(nameB));
    assert.strictEqual(await sidebar.isSandboxActive(nameA), false, "Two sandboxes are highlighted in the sidebar.");
  });

  it("hides an active pane behind a page, keeps it alive, and restores it from the sidebar", async () => {
    const chatLabel = await openChat(nameA);
    assert.strictEqual(await sandboxHeader.sandboxInView(), nameA, "Opening a pane by command did not bring its sandbox into view.");
    assert(await sandboxHeader.isTabActive(nameA, "agentchat"), "The chat tab is not current after opening the chat.");

    await ModelsPage.open();
    const onPage = await workspaceState();
    assert.strictEqual(onPage.active_app, null, "Showing a page did not hide the active pane.");
    assert(paneFor(onPage, nameA, "agentchat"), "Showing a page closed the pane instead of hiding it.");
    assert.strictEqual(await sandboxHeader.sandboxInView(), null, "The sandbox header stayed over the Models page.");
    assert.strictEqual(await (await chat.root(nameA)).isExisting(), false, "The chat view stayed mounted over the Models page.");

    // The sandbox's last view was the chat, so its sidebar entry restores the chat, not Manage.
    await sidebar.selectSandbox(nameA);
    await waitForActiveApp(chatLabel, "Selecting the sandbox in the sidebar did not restore its chat pane.");
    await (await chat.root(nameA)).waitForExist({ timeout: PANEL_RENDER });
    await chat.waitForConnection(nameA, "attached");
    assert(await sandboxHeader.isTabActive(nameA, "agentchat"));
  });

  it("the Home card still opens Manage when the last view was a pane", async () => {
    await ProjectsPage.row(nameA).openWorkspace();
    await workspaceDashboard.waitForRendered(nameA);
    const state = await workspaceState();
    assert.strictEqual(state.active_app, null, "The Home card restored the pane instead of opening Manage (Resolved Decision 2).");
    assert(paneFor(state, nameA, "agentchat"), "Opening Manage from the card closed the chat pane.");
    assert(await sandboxHeader.isTabActive(nameA, "dashboard"));
  });

  it("the sidebar restores Manage when Manage was the last view", async () => {
    await SettingsPage.open();
    await sidebar.selectSandbox(nameA);
    await workspaceDashboard.waitForRendered(nameA);
    assert.strictEqual((await workspaceState()).active_app, null, "The sidebar restored the chat although Manage was the last view.");
    assert(await sandboxHeader.isTabActive(nameA, "dashboard"));
  });

  it("collapses the sidebar to the rail and keeps navigating", async () => {
    await sidebar.setCollapsed(true);
    assert.strictEqual((await workspaceState()).collapsed, true, "The rail state did not reach Rust (set_panel_collapsed).");
    const settings = await navButton("/settings");
    assert.strictEqual((await settings.getText()).trim(), "", "A rail entry still shows its text label.");
    assert(await settings.getAttribute("aria-label"), "A rail entry lost its accessible name.");

    await SettingsPage.open();
    await sidebar.selectSandbox(nameA);
    await workspaceDashboard.waitForRendered(nameA);

    await sidebar.setCollapsed(false);
    assert.strictEqual((await workspaceState()).collapsed, false);
    assert.notStrictEqual((await (await navButton("/settings")).getText()).trim(), "", "The expanded entry shows no label.");
  });

  it("New sandbox goes Home and focuses the create card's name field", async () => {
    assert.strictEqual(await sandboxHeader.sandboxInView(), nameA, "Precondition: a sandbox is in view.");
    await sidebar.newSandbox();
    await (await ProjectsPage.root).waitForDisplayed({ timeout: UI_SETTLE });
    const input = await ProjectsPage.nameInput;
    await browser.waitUntil(async () => browser.execute((el) => document.activeElement === el, input), {
      timeout: UI_SETTLE,
      timeoutMsg: "The create card's name field never took the focus after New sandbox.",
    });
  });

  it("closing a sandbox whose pane is active hands the view to the remaining sandbox's pane", async () => {
    const chatB = await openChat(nameB);
    await openChat(nameA);
    assert.strictEqual(await sandboxHeader.sandboxInView(), nameA);

    await sandboxHeader.closeSandbox(nameA);
    await waitForActiveApp(chatB, "Closing sandbox A did not activate sandbox B's chat as the neighbour.");
    await (await sandboxHeader.root(nameB)).waitForDisplayed({
      timeout: UI_SETTLE,
      timeoutMsg: "Sandbox B did not come into view after closing A (Resolved Decision 6).",
    });
    assert.strictEqual(panesFor(await workspaceState(), nameA).length, 0, "Closing sandbox A left one of its panes open.");
  });

  it("closing the sandbox in view lands on Home", async () => {
    await sandboxHeader.closeSandbox(nameB);
    await (await ProjectsPage.root).waitForDisplayed({ timeout: UI_SETTLE, timeoutMsg: "Closing the sandbox did not land on Home." });
    assert.strictEqual(new URL(await browser.getUrl()).pathname, "/projects");
    assert(await isPageShown("/projects"), "Home is showing but its sidebar entry is not highlighted.");
    assert.strictEqual(await sandboxHeader.sandboxInView(), null);
    const state = await workspaceState();
    assert.strictEqual(state.apps.length, 0, `Panes survived closing every sandbox: ${state.apps.map((p) => p.label).join(", ")}`);
    assert.strictEqual(state.active_app, null);
  });

  it("deleting a sandbox from Manage lands on Home without it", async () => {
    await ProjectsPage.row(nameB).destroy();
    assert(await (await ProjectsPage.root).isDisplayed(), "Delete did not end on Home.");
    assert.strictEqual(await sandboxHeader.sandboxInView(), null);
    await (await sidebar.sandboxItem(nameB)).waitForExist({
      timeout: COMMAND,
      reverse: true,
      timeoutMsg: "The deleted sandbox is still listed in the sidebar.",
    });
  });
});
