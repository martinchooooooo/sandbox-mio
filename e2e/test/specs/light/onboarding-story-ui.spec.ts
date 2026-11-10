import assert from "node:assert";
import path from "node:path";

import { configureMock, invokeCmd, useMockControl } from "../../../lib/mock.js";
import { COMMAND, UI_SETTLE } from "../../../lib/timeouts.js";

// LIGHT spec for the onboarding story: the book mounts and turns pages, first-run setup starts in
// the background when the story opens, and the activity toggle reports it. The mock backend answers
// instantly, so only the end states (finished, needs attention) are observable here.

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
  const begin = await $("[data-welcome-begin]");
  await begin.waitForClickable({ timeout: UI_SETTLE });
  await begin.click();
  await $("[data-story-book]").waitForDisplayed({ timeout: UI_SETTLE });
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

  it("plays the story while setup finishes in the background, then lets the user finish", async () => {
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
    const finish = await $("[data-welcome-finish]");
    await finish.waitForEnabled({ timeout: COMMAND });
    await shot("03-setup-cards");
    await finish.click();
    await $("[role='tab'][data-value='/projects']").waitForDisplayed({ timeout: COMMAND });
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
      await (await $("[data-welcome-finish]")).isEnabled(),
      false,
      "Finish is enabled although the base image is not built.",
    );
  });
});
