import assert from "node:assert";

import SettingsPage from "../../pageobjects/settings.page.js";
import { invokeCmd } from "../../../lib/mock.js";
import { COMMAND, UI_SETTLE } from "../../../lib/timeouts.js";

const FIRST_SANDBOX = "hello-world";

async function sandboxNames(): Promise<string[]> {
  return (await invokeCmd<{ name: string }[]>("list_sandboxes")).map((p) => p.name);
}

// LIGHT spec for Settings → "Advanced Debugging": proves reset_stuck_podman_machine round-trips
// against the real PodmanMachineManager, with `wsl --unregister` gated off under MIO_MOCK_BACKEND=1
// (run_host_wsl) to avoid nuking the dev's machine — that WSL path is covered by MachineManager::reset_stuck_machine's docs instead.

describe("Settings — Advanced Debugging (no backend task)", function () {
  this.timeout(60000);

  it("resets the Podman machine and shows the done confirmation", async () => {
    await SettingsPage.goHome();
    await SettingsPage.open();
    await SettingsPage.resetPodmanMachine();
    assert(
      await (await SettingsPage.resetPodmanMachineDoneMarker).isDisplayed(),
      "Reset confirmation did not appear — reset_stuck_podman_machine did not resolve or the UI did not update.",
    );
  });

  it("restarts onboarding and deletes hello-world only when the switch is on", async () => {
    await invokeCmd("create_sandbox", { name: FIRST_SANDBOX, proxy: false });
    try {
      await SettingsPage.goHome();
      await SettingsPage.open();
      await SettingsPage.restartOnboarding(false);
      await $("[data-welcome-step]").waitForDisplayed({ timeout: COMMAND });
      assert(
        (await sandboxNames()).includes(FIRST_SANDBOX),
        "hello-world was deleted although the restart's delete switch was off.",
      );

      // The restart cleared the completed flag; restore it so the nav chrome is back for the next steps.
      await invokeCmd("set_settings", { settings: { ...(await invokeCmd<object>("get_settings")), onboardingCompleted: true } });
      await browser.refresh();
      await SettingsPage.goHome();
      await SettingsPage.open();
      await SettingsPage.restartOnboarding(true);
      await $("[data-welcome-step]").waitForDisplayed({ timeout: COMMAND });
      await browser.waitUntil(async () => !(await sandboxNames()).includes(FIRST_SANDBOX), {
        timeout: UI_SETTLE,
        timeoutMsg: "hello-world survived a restart with the delete switch on.",
      });
    } finally {
      await invokeCmd("destroy_sandbox", { name: FIRST_SANDBOX, force: true }).catch(() => undefined);
      // Other specs depend on the nav chrome that a completed onboarding unlocks.
      await invokeCmd("set_settings", { settings: { ...(await invokeCmd<object>("get_settings")), onboardingCompleted: true } });
      await browser.refresh();
    }
  });
});
