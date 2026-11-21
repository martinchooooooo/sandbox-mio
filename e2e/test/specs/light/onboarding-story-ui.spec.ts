import assert from "node:assert";
import path from "node:path";

import { navButton } from "../../pageobjects/app.page.js";
import { configureMock, invokeCmd, useMockControl } from "../../../lib/mock.js";
import { COMMAND, UI_SETTLE } from "../../../lib/timeouts.js";

// LIGHT spec for the onboarding story: the book mounts and turns pages, first-run setup starts in
// the background when the story opens, and the activity toggle reports it. The mock backend answers
// machine and image setup instantly, so only the end states (finished, needs attention) are
// observable here. The spec declines the model ("Set up without a Model") because inference has no
// mock backend; the real model path (download, serve, pi in the sandbox) belongs in a standard or
// heavy tier spec.

// Set MIO_E2E_SHOTS to a directory to keep a screenshot of each stage for a visual check.
const SHOTS = process.env.MIO_E2E_SHOTS;
async function shot(name: string): Promise<void> {
  if (SHOTS) await browser.saveScreenshot(path.join(SHOTS, `${name}.png`));
}

async function setOnboarded(value: boolean): Promise<void> {
  const settings = await invokeCmd<Record<string, unknown>>("get_settings");
  await invokeCmd("set_settings", { settings: { ...settings, onboardingCompleted: value } });
  await browser.refresh();
}

async function openStory(): Promise<void> {
  const start = await $("[data-welcome-get-started]");
  await start.waitForClickable({ timeout: COMMAND });
  await start.click();
  // Decline the model: confirming would download and serve a real one, which the mock backend cannot fake.
  const decline = await $("[data-welcome-decline]");
  await decline.waitForClickable({ timeout: UI_SETTLE });
  await decline.click();
  await $("[data-story-book]").waitForDisplayed({ timeout: UI_SETTLE });
}

async function removeFirstSandbox(): Promise<void> {
  await invokeCmd("destroy_sandbox", { name: "hello-world", force: true }).catch(() => undefined);
}

async function sandboxNames(): Promise<string[]> {
  return (await invokeCmd<{ name: string }[]>("list_sandboxes")).map((p) => p.name);
}

async function skipTour(): Promise<void> {
  const skip = await $("[data-welcome-skip]");
  await skip.waitForClickable({ timeout: UI_SETTLE });
  await skip.click();
  await $("[data-welcome-step='actions']").waitForDisplayed({ timeout: UI_SETTLE });
}

async function headlineState(): Promise<string | null> {
  const toggle = await $("[data-activity-headline]");
  return (await toggle.isExisting()) ? toggle.getAttribute("data-activity-headline") : null;
}

describe("Onboarding story — book and background setup", function () {
  this.timeout(120000);
  useMockControl();

  // The harness seeds the flag true; every other spec depends on the nav chrome it unlocks.
  after(async () => {
    await setOnboarded(true);
  });

  it("plays the story while setup finishes in the background, then hands over to the hello-world sandbox", async () => {
    await setOnboarded(false);
    await openStory();

    const book = await $("[data-story-book]");
    assert.strictEqual(
      await book.getAttribute("data-story-pages"),
      "16",
      "The book did not register the chapter's 16 pages — a <Page> failed to register, or the chapter list in src/lib/onboarding/chapters.ts changed.",
    );
    await browser.waitUntil(async () => (await $("[data-story-caption]").getText()).length > 0, {
      timeout: COMMAND,
      timeoutMsg: "No caption appeared — the first scene's script did not run (check the console for an error thrown inside play()).",
    });
    await shot("01-story-first-page");

    await browser.waitUntil(async () => (await headlineState()) === "done", {
      timeout: COMMAND,
      timeoutMsg: "The activity toggle never reported setup as done — startBackgroundSetup() did not run or did not reach the image step.",
    });

    await $("[data-story-next]").click();
    await browser.waitUntil(async () => (await book.getAttribute("data-story-page")) === "1", {
      timeout: UI_SETTLE,
      timeoutMsg: "Next did not turn the page.",
    });
    await $("[data-story-dot='11']").click();
    await browser.waitUntil(async () => (await book.getAttribute("data-story-page")) === "11", {
      timeout: UI_SETTLE,
      timeoutMsg: "A page dot did not jump to its page.",
    });
    // Long enough for the sandbox to appear on the host's screen.
    await browser.pause(6000);
    await shot("02-story-sandbox-page");

    await $("[data-welcome-skip]").click();
    // The background setup has built hello-world by now (the first sandbox is part of it), so the
    // progress page hands over at once and may be gone before it can be observed; it is only
    // captured when it is still up.
    if (SHOTS && (await $("[data-setup-progress]").isExisting())) await shot("03-setup-cards");
    // Everything ready hands over to the hello-world sandbox, which also finishes onboarding.
    // Finishing unlocks the app chrome, whose sidebar always carries the Home entry.
    await navButton("/projects").waitForDisplayed({ timeout: COMMAND });

    // The model was declined, so hello-world exists with no agent installed.
    const first = (await invokeCmd<{ name: string; workspace_path: string }[]>("list_sandboxes")).find((p) => p.name === "hello-world");
    assert(first, "hello-world was not created by the first-run setup.");
    assert.deepStrictEqual(
      await invokeCmd<string[]>("list_packages", { workspacePath: first.workspace_path }),
      [],
      "An agent was installed into hello-world although the model was declined.",
    );
  });

  it("offers a retry when the first sandbox fails and hands over once it works", async () => {
    await removeFirstSandbox();
    await configureMock("podman.create", { kind: "fail-once", exit_code: 1, stderr: "create boom (scripted)" });
    await setOnboarded(false);
    await openStory();
    await skipTour();

    const retry = await $("[data-first-sandbox-retry]");
    await retry.waitForDisplayed({ timeout: COMMAND });
    await $("[data-first-sandbox-skip]").waitForDisplayed({ timeout: UI_SETTLE });
    await retry.click();
    await navButton("/projects").waitForDisplayed({ timeout: COMMAND });
    assert((await sandboxNames()).includes("hello-world"), "The retry did not create hello-world.");
  });

  it("lets the user leave the progress page without opening the sandbox", async () => {
    await removeFirstSandbox();
    await configureMock("podman.create", { kind: "delay", ms: 3000, calls: 1 });
    await setOnboarded(false);
    await openStory();
    await skipTour();

    const cancel = await $("[data-welcome-cancel]");
    await cancel.waitForClickable({ timeout: UI_SETTLE });
    await cancel.click();
    await navButton("/projects").waitForDisplayed({ timeout: COMMAND });

    await browser.waitUntil(async () => (await sandboxNames()).includes("hello-world"), {
      timeout: COMMAND,
      timeoutMsg: "The create stopped when the user left the progress page; it must keep running in the background.",
    });
    await browser.pause(2000);
    const state = await invokeCmd<{ apps: { sandbox: string }[] }>("workspace_state");
    assert(
      !state.apps.some((a) => a.sandbox === "hello-world"),
      "hello-world was opened automatically after the user chose to go to the sandbox list.",
    );
  });

  it("flags a failed background step and offers a way to the setup cards", async () => {
    await configureMock("podman.image-exists", { kind: "fail-always", exit_code: 1, stderr: "" });
    await configureMock("podman.build", { kind: "fail-always", exit_code: 125, stderr: "build failed (e2e)" });
    await setOnboarded(false);
    await openStory();

    await browser.waitUntil(async () => (await headlineState()) === "attention", {
      timeout: COMMAND,
      timeoutMsg: "A failed image build did not turn the activity toggle to its attention state.",
    });
    await shot("04-attention-pill");
    await $("[data-activity-toggle]").click();
    const detail = await $("[data-activity-headline-detail]");
    await detail.waitForDisplayed({ timeout: UI_SETTLE });
    await shot("05-attention-panel");
    await detail.$("button").click();
    await $("[data-welcome-step='actions']").waitForDisplayed({ timeout: UI_SETTLE });
    assert.strictEqual(
      await (await $("[data-welcome-handoff]")).isExisting(),
      false,
      "The first-sandbox handoff started although the base image is not built.",
    );
  });
});
