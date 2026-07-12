import assert from "node:assert";

import SettingsPage from "../../pageobjects/settings.page.js";

// LIGHT spec for the Settings → "Advanced Debugging" section: proves the reset_stuck_podman_machine
// command round-trips against the mock MachineManager (always Ok — see machine/mock.rs) and the UI
// reflects completion. The real WSL/podman-side behaviour (unregistering a stuck machine's WSL
// distro) is not exercisable in CI and is covered by the doc comments on
// MachineManager::reset_stuck_machine in src-tauri/src/machine/types.rs instead.

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
