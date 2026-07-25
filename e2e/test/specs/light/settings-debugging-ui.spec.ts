import assert from "node:assert";

import SettingsPage from "../../pageobjects/settings.page.js";

// LIGHT spec for Settings → "Advanced Debugging": proves reset_stuck_podman_machine round-trips
// against the real PodmanMachineManager. `wsl --unregister` is gated off under MIO_MOCK_BACKEND=1
// (run_host_wsl, src-tauri/src/machine/mod.rs) — without that gate this would delete the dev's
// podman machine; the skipped WSL path is covered by MachineManager::reset_stuck_machine's doc comments instead.

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
