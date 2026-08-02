import assert from "node:assert";

import SettingsPage from "../../pageobjects/settings.page.js";

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
});
