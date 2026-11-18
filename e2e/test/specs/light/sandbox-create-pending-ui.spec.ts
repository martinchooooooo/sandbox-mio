import ProjectsPage from "../../pageobjects/projects.page.js";
import { showHome } from "../../pageobjects/app.page.js";
import { configureMock, useMockControl } from "../../../lib/mock.js";

// LIGHT spec for background creation (src/lib/sandbox/lifecycle.svelte.ts): a create shows a
// placeholder in the sidebar and on Home while `podman create` is held, then the app opens the new
// sandbox; a scripted failure leaves the placeholder with the error plus Retry and Dismiss.
describe("Sandbox create — placeholder while creating (mock backend)", function () {
  this.timeout(90000);

  useMockControl();

  const sidebarEntry = (name: string) => $(`[data-nav-sandbox="${name}"]`);
  const placeholder = (name: string) => $(`li[data-sandbox="${name}"][data-pending="create"]`);

  async function submit(name: string) {
    await showHome();
    await (await ProjectsPage.nameInput).setValue(name);
    await (await ProjectsPage.createButton).click();
  }

  it("shows the placeholder in the sidebar and on Home, then opens the new sandbox", async () => {
    const name = `e2e-create-${Date.now()}`;
    await ProjectsPage.open();
    await ProjectsPage.ensurePodmanReady();

    await configureMock("podman.create", { kind: "delay", ms: 4000, calls: 1 });
    await submit(name);

    await sidebarEntry(name).waitForExist({ timeout: 5000, timeoutMsg: "no sidebar placeholder" });
    await expect(sidebarEntry(name)).toHaveAttribute("data-pending", "create");
    await placeholder(name).waitForDisplayed({ timeout: 5000, timeoutMsg: "no Home placeholder" });

    await $(`div[data-sandbox-header][data-sandbox="${name}"]`).waitForDisplayed({ timeout: 30000 });
    await browser.waitUntil(async () => (await sidebarEntry(name).getAttribute("data-pending")) === null, {
      timeout: 5000,
      timeoutMsg: "sidebar entry still pending after create",
    });
  });

  it("keeps the error with Retry and Dismiss when the create fails", async () => {
    const name = `e2e-create-fail-${Date.now()}`;
    await ProjectsPage.open();

    await configureMock("podman.create", { kind: "fail-once", exit_code: 1, stderr: "create boom (scripted)" });
    await submit(name);

    await showHome();
    const card = placeholder(name);
    await card.waitForDisplayed({ timeout: 30000 });
    await card.$("p*=create boom").waitForDisplayed({ timeout: 30000, timeoutMsg: "error not shown" });
    await expect(card.$("[data-create-dismiss]")).toBeDisplayed();

    await card.$("[data-create-retry]").click();
    await $(`div[data-sandbox-header][data-sandbox="${name}"]`).waitForDisplayed({ timeout: 30000 });

    // Dismiss path
    const name2 = `${name}-b`;
    await configureMock("podman.create", { kind: "fail-once", exit_code: 1, stderr: "create boom (scripted)" });
    await submit(name2);
    await showHome();
    await placeholder(name2).$("[data-create-dismiss]").waitForDisplayed({ timeout: 30000 });
    await placeholder(name2).$("[data-create-dismiss]").click();
    await placeholder(name2).waitForExist({ reverse: true, timeout: 5000 });
  });
});
