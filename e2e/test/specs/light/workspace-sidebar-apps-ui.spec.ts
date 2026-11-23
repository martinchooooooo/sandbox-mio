import assert from "node:assert";

import ProjectsPage from "../../pageobjects/projects.page.js";
import sidebar from "../../pageobjects/sidebar.page.js";
import sandboxView from "../../pageobjects/sandbox-view.page.js";
import workspaceDashboard from "../../pageobjects/workspace-dashboard.page.js";
import { captureDiagOnFailure } from "../../../lib/diag.js";
import { configureMock, invokeCmd, useMockControl } from "../../../lib/mock.js";
import { COMMAND, PANEL_RENDER, UI_SETTLE } from "../../../lib/timeouts.js";
import { paneFor, workspaceState } from "../../../lib/workspace.js";

// LIGHT spec for the sidebar-only chrome (no sandbox header bar): the apps group under the sandbox in
// view (only apps that are open), the Overview app cards that launch them, the Dashboard title bar's
// Start/Stop/menu/delete flow, and the always-labelled section tabs. Terminal is the native terminal
// pane, which the mock terminal host serves without a webview.

describe("Sidebar apps group and Dashboard title bar (mock backend)", function () {
  this.timeout(120000);

  captureDiagOnFailure();
  useMockControl();

  const nameA = `e2e-apps-a-${Date.now()}`;
  const nameB = `e2e-apps-b-${Date.now()}`;

  after(async () => {
    await invokeCmd("set_panel_collapsed", { collapsed: false }).catch(() => undefined);
    for (const name of [nameA, nameB]) {
      await invokeCmd("close_workspace", { name }).catch(() => undefined);
      await invokeCmd("destroy_sandbox", { name, force: true }).catch(() => undefined);
    }
  });

  it("shows the apps group only for the sandbox in view, with Dashboard active on open", async () => {
    await ProjectsPage.open();
    await ProjectsPage.ensurePodmanReady();
    await ProjectsPage.createProject(nameA);
    await ProjectsPage.createProject(nameB);

    await ProjectsPage.row(nameA).openWorkspace();
    await workspaceDashboard.waitForRendered(nameA);

    assert(await (await sandboxView.appsGroup(nameA)).isDisplayed(), "The in-view sandbox has no apps group.");
    assert.strictEqual(await (await sandboxView.appsGroup(nameB)).isExisting(), false, "A sandbox that is not in view has an apps group.");
    assert(await sandboxView.isTabActive(nameA, "dashboard"), "The Dashboard child is not active on open.");
    assert.strictEqual(
      await (await sidebar.sandboxItem(nameA)).getAttribute("data-active"),
      "false",
      "The in-view sandbox row is filled instead of only outlined.",
    );
    assert.strictEqual(
      await (await sandboxView.tab(nameA, "terminal")).isExisting(),
      false,
      "The sidebar lists Terminal before it was opened.",
    );
    const card = await workspaceDashboard.launcherButton("terminal");
    assert(await card.isDisplayed(), "The Terminal card is missing from the Overview.");
    assert(await card.isEnabled(), "The Terminal card is disabled.");
    const chatCard = await workspaceDashboard.launcherButton("agentchat");
    assert(await chatCard.isDisplayed(), "The unavailable Chat card is hidden instead of disabled.");
    assert.strictEqual(await chatCard.isEnabled(), false, "Chat is enabled without pi.");
    assert(await (await sandboxView.appsGroup(nameA).$("button[data-open-folder]")).isDisplayed(), "The Open folder child is missing.");

    await sidebar.selectSandbox(nameB);
    await workspaceDashboard.waitForRendered(nameB);
    assert.strictEqual(await (await sandboxView.appsGroup(nameA)).isExisting(), false, "Sandbox A kept its apps group after B came into view.");
    assert(await (await sandboxView.appsGroup(nameB)).isDisplayed());
  });

  it("launches Terminal from its card, lists it in the sidebar, and closes it from the card", async () => {
    await workspaceDashboard.waitForRendered(nameB);
    await sandboxView.launchFromCard(nameB, "terminal");
    await browser.waitUntil(async () => paneFor(await workspaceState(), nameB, "terminal") !== undefined, {
      timeout: COMMAND,
      timeoutMsg: "launching Terminal from its card did not open a terminal pane",
    });
    await browser.waitUntil(async () => sandboxView.isTabActive(nameB, "terminal"), {
      timeout: COMMAND,
      timeoutMsg: "the Terminal child never became active",
    });
    assert.strictEqual(await sandboxView.isTabActive(nameB, "dashboard"), false, "Dashboard stays active over a pane.");
    assert.strictEqual(await (await sandboxView.titleBar).isExisting(), false, "The title bar shows over a pane.");
    assert(await (await sandboxView.closeTabButton(nameB, "terminal")).isExisting(), "The open Terminal child has no close button.");

    await sandboxView.clickTab(nameB, "dashboard");
    await (await sandboxView.titleBar).waitForDisplayed({ timeout: PANEL_RENDER });
    assert.strictEqual((await workspaceState()).active_app, null, "Dashboard did not hide the pane.");
    assert(await (await sandboxView.tab(nameB, "terminal")).isDisplayed(), "The open Terminal left the sidebar with the Dashboard in view.");

    await sandboxView.closeTab(nameB, "terminal");
    await browser.waitUntil(async () => paneFor(await workspaceState(), nameB, "terminal") === undefined, {
      timeout: COMMAND,
      timeoutMsg: "the sidebar close button did not close the terminal pane",
    });
    await (await sandboxView.tab(nameB, "terminal")).waitForExist({ timeout: UI_SETTLE, reverse: true, timeoutMsg: "A closed app stayed in the sidebar." });

    await sandboxView.ensureDashboard(nameB);
    await workspaceDashboard.launchApp("terminal");
    await browser.waitUntil(async () => paneFor(await workspaceState(), nameB, "terminal") !== undefined, { timeout: COMMAND });
    await sandboxView.clickTab(nameB, "dashboard");
    await (await sandboxView.titleBar).waitForDisplayed({ timeout: PANEL_RENDER });
    const closeCard = await workspaceDashboard.closeAppButton("terminal");
    await closeCard.waitForClickable({ timeout: PANEL_RENDER });
    await closeCard.click();
    await browser.waitUntil(async () => paneFor(await workspaceState(), nameB, "terminal") === undefined, {
      timeout: COMMAND,
      timeoutMsg: "the card's Close button did not close the terminal pane",
    });
    await (await sandboxView.tab(nameB, "terminal")).waitForExist({ timeout: UI_SETTLE, reverse: true, timeoutMsg: "Closing from the card left Terminal in the sidebar." });
  });

  it("switches to an already-open app from its card", async () => {
    await sandboxView.launchFromCard(nameB, "terminal");
    await browser.waitUntil(async () => sandboxView.isTabActive(nameB, "terminal"), { timeout: COMMAND });
    await sandboxView.clickTab(nameB, "dashboard");
    await (await sandboxView.titleBar).waitForDisplayed({ timeout: PANEL_RENDER });
    await workspaceDashboard.openSection("overview");
    await workspaceDashboard.launchApp("terminal");
    await browser.waitUntil(async () => sandboxView.isTabActive(nameB, "terminal"), {
      timeout: COMMAND,
      timeoutMsg: "clicking an open app's card did not switch to its pane",
    });
    await sandboxView.closeTab(nameB, "terminal");
    await sandboxView.ensureDashboard(nameB);
  });

  it("opens the sandbox folder from the title bar and the sidebar without an error", async () => {
    await sandboxView.ensureDashboard(nameB);
    const titleBarFolder = await $("[data-sandbox-title-bar] [data-open-folder]");
    await titleBarFolder.waitForClickable({ timeout: PANEL_RENDER });
    await titleBarFolder.click();
    const sidebarFolder = await sandboxView.appsGroup(nameB).$("[data-open-folder]");
    await sidebarFolder.waitForClickable({ timeout: PANEL_RENDER });
    await sidebarFolder.click();
    await browser.pause(UI_SETTLE);
    assert.strictEqual(await (await sandboxView.errorRow()).isExisting(), false, "Open folder left an error in the title bar.");
    assert.strictEqual(await (await $("[data-sonner-toast][data-type='error']")).isExisting(), false, "Open folder raised an error toast.");
  });

  it("labels the section tabs with visible text on one row while the sidebar is expanded", async () => {
    await invokeCmd("set_panel_collapsed", { collapsed: false });
    await sandboxView.ensureDashboard(nameB);
    await workspaceDashboard.waitForRendered(nameB);
    const tops = new Set<number>();
    for (const id of ["overview", "tools", "ports", "network", "traffic", "remote"] as const) {
      const item = await workspaceDashboard.sectionItem(id);
      assert(await item.isDisplayed(), `Section tab ${id} is not displayed.`);
      assert.notStrictEqual((await item.getText()).trim(), "", `Section tab ${id} has no visible text label.`);
      tops.add((await item.getLocation("y")) as number);
    }
    assert.strictEqual(tops.size, 1, "The section tabs wrap onto more than one row at the default window size.");
    assert.strictEqual(await (await workspaceDashboard.sectionItem("overview")).$("svg").isDisplayed(), false, "Tab icons show although the strip is narrow.");
  });

  it("shows Start while stopped and Stop beside the status once running", async () => {
    await sandboxView.ensureDashboard(nameB);
    assert(await (await sandboxView.startButton).isDisplayed(), "A stopped sandbox has no Start button.");
    assert.strictEqual(await (await sandboxView.stopButton).isExisting(), false, "A stopped sandbox offers Stop.");
    await sandboxView.openMenu();
    assert.strictEqual(await (await sandboxView.restartItem).isExisting(), false, "A stopped sandbox offers Restart.");
    assert(await (await sandboxView.closeMenuItem).isDisplayed(), "The menu has no Close sandbox item.");
    assert(await (await sandboxView.destroyItem).isDisplayed(), "The menu has no Delete sandbox item.");
    await browser.keys("Escape");

    await sandboxView.start(nameB);
    await sandboxView.waitForRunning(nameB);
    await (await sandboxView.startButton).waitForExist({ timeout: UI_SETTLE, reverse: true, timeoutMsg: "Start stayed after the sandbox started." });
    assert(await (await sandboxView.stopButton).isDisplayed(), "A running sandbox has no visible Stop button on the Dashboard.");
    await sandboxView.openMenu();
    assert(await (await sandboxView.restartItem).isDisplayed(), "A running sandbox has no Restart in the menu.");
    assert.strictEqual(await (await $("[role='menu'] [data-sandbox-stop]")).isExisting(), false, "Stop is still in the menu.");
    await browser.keys("Escape");

    await sandboxView.stop(nameB);
    await sandboxView.waitForStopped(nameB);
    await (await sandboxView.startButton).waitForDisplayed({ timeout: UI_SETTLE, timeoutMsg: "Start did not return after Stop." });
  });

  it("disables Delete while a lifecycle operation is pending", async () => {
    await sandboxView.ensureDashboard(nameB);
    await configureMock("podman.start", { kind: "delay", ms: 4000, calls: 1 });
    await sandboxView.start(nameB);
    await sandboxView.openMenu();
    assert.strictEqual(
      await (await sandboxView.destroyItem).getAttribute("aria-disabled"),
      "true",
      "Delete sandbox stays enabled while Start is pending.",
    );
    await browser.keys("Escape");
    await sandboxView.waitForRunning(nameB);
  });

  it("lists no Delete tab and confirms deletion in a dialog that Cancel dismisses", async () => {
    await sandboxView.ensureDashboard(nameB);
    assert.strictEqual(await (await $("[data-section-nav-item='danger']")).isExisting(), false, "A Delete section tab is still listed.");

    await sandboxView.openDestroyDialog(nameB);
    assert((await (await sandboxView.destroyDialog).getText()).includes(nameB), "The delete dialog does not name the sandbox.");
    await (await sandboxView.destroyCancel).click();
    await (await sandboxView.destroyDialog).waitForExist({ timeout: UI_SETTLE, reverse: true, timeoutMsg: "Cancel left the delete dialog open." });
    assert(await (await sidebar.sandboxItem(nameB)).isExisting(), "Cancel deleted the sandbox.");
  });

  it("closes the sandbox from the title bar menu", async () => {
    await sandboxView.ensureDashboard(nameB);
    await sandboxView.openMenu();
    await (await sandboxView.closeMenuItem).click();
    await (await ProjectsPage.root).waitForDisplayed({ timeout: UI_SETTLE, timeoutMsg: "Closing from the menu did not land on Home." });
    assert.strictEqual(await sandboxView.sandboxInView(), null);
  });

  it("deletes the sandbox from the menu through the confirmation dialog", async () => {
    await sandboxView.destroy(nameA);
    await browser.waitUntil(async () => !(await (await sidebar.sandboxItem(nameA)).isExisting()), {
      timeout: COMMAND,
      timeoutMsg: "Confirming the delete dialog did not remove the sandbox from the sidebar.",
    });
    assert.strictEqual(await sandboxView.sandboxInView(), null, "The deleted sandbox stayed in view.");
  });
});
