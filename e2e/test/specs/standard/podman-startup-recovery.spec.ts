import assert from "node:assert";
import { spawnSync } from "child_process";

import ProjectsPage from "../../pageobjects/projects.page.js";
import SettingsPage from "../../pageobjects/settings.page.js";
import ActivityCenter from "../../pageobjects/activity.page.js";
import { podman, LOCAL_CONNECTION } from "../../../lib/podman-connection.js";
import { activatePodmanSocket, DEFAULT_MACHINE } from "../../../lib/podman-machine.js";
import { COMMAND, PANEL_RENDER, PODMAN_RECOVERY, SUBPROCESS, UI_SETTLE } from "../../../lib/timeouts.js";
import { captureDiagOnFailure } from "../../../lib/diag.js";

// Proves the Podman startup-recovery feature end to end (docs/features/podman-startup-recovery):
// stopping podman-machine-default surfaces the recovery screen, and "Start Podman" must run the
// real check_podman_ready/start_podman_machine path, not just report Running:true (Step 6 is decisive).
//
// Takes 30-60s for the stop/start cycle (no artefact downloads), hence the widened 120s timeout.
//
// Sandboxes target mio-tests (kept running throughout), so check_other_machines_running() skips the
// `wsl --shutdown` reset — that path is out of scope here (see the e2e plan's Environment Notes).
describe("Podman startup recovery", function () {
  this.timeout(120000);

  captureDiagOnFailure();

  before(() => {
    const stop = spawnSync("podman", ["machine", "stop", DEFAULT_MACHINE], {
      encoding: "utf8",
      shell: true,
    });
    // A non-zero exit means the machine was already stopped or does not exist — treat as OK so a
    // re-run after a failed recovery still proceeds (the recovery screen needs only a down machine).
    if (stop.status !== 0) {
      console.warn(
        `[e2e] podman machine stop ${DEFAULT_MACHINE} exited ${stop.status}: ${stop.stderr.trim()}. ` +
          "Proceeding — the machine may already be stopped.",
      );
    }
  });

  after(() => {
    // Restarting alone isn't enough: check_podman_ready also polls the rootless socket
    // (src-tauri/src/machine/mod.rs), so a Running machine with a dead socket still fails the probe —
    // and a mid-spec failure here would leave every later spec hitting the recovery screen. Warn, don't throw.
    spawnSync("podman", ["machine", "start", DEFAULT_MACHINE], {
      encoding: "utf8",
      shell: true,
    });
    if (!activatePodmanSocket(DEFAULT_MACHINE)) {
      console.warn(
        `[e2e] ${DEFAULT_MACHINE}'s rootless socket did not come up after this spec. ` +
          "Specs following this one will show the 'Start Podman' recovery screen.",
      );
    }
  });

  // Step 1 — Leave Home so its onMount fires fresh on the next step. Home probes Podman only when
  // it mounts (entered from another page or from a sandbox), so staying on Home would not re-probe.
  it("leaves Home so the Home onMount fires fresh on the next step", async () => {
    await SettingsPage.open();
    await (await ProjectsPage.root).waitForExist({ timeout: UI_SETTLE, reverse: true });
  });

  // Step 2 — Entering Home probes check_podman_ready, which detects the stopped machine and
  // renders the recovery screen.
  it("shows the 'Start Podman' screen when the machine is stopped", async () => {
    await ProjectsPage.open();
    await ProjectsPage.startPodmanButton.waitForDisplayed({
      timeout: PANEL_RENDER,
      timeoutMsg:
        "Expected 'Start Podman' button but it did not appear. check_podman_ready may have " +
        "returned Ok (machine still running) or the button label may have changed.",
    });
    await ProjectsPage.podmanNotRunningText.waitForDisplayed({
      timeout: UI_SETTLE,
      timeoutMsg: "Explanation text 'Podman machine is not running' not visible alongside the Start Podman button.",
    });
  });

  // Step 3 — Clicking Start Podman disables the button and marks it busy for the duration of
  // start_podman_machine.
  it("clicking Start Podman enters a loading state", async () => {
    const startButton = await ProjectsPage.startPodmanButton;
    await startButton.click();
    await browser.waitUntil(
      async () => {
        const busy = await startButton.getAttribute("aria-busy");
        const disabled = await startButton.getAttribute("disabled");
        return busy === "true" || disabled !== null;
      },
      {
        timeout: UI_SETTLE,
        interval: 200,
        timeoutMsg: "Button did not enter a loading/disabled state after click.",
      },
    );
  });

  // Step 4 — start_podman_machine called activity::open("podman-startup", …); the task reaches the
  // frontend store and renders as a row in the activity panel. Accepts running or succeeded: the
  // panel poll may first observe the task after it has already completed.
  it("activity center shows the 'podman-startup' task", async () => {
    await ActivityCenter.openPanel();
    await ActivityCenter.waitForTask("podman-startup", ["running", "succeeded"], SUBPROCESS);
  });

  // Step 5 — Recovery completes when startPodman() clears the podmanUnavailable flag. The Start Podman
  // button's disappearance is the discriminating signal — Create stays mounted throughout, so its
  // presence alone would not prove recovery.
  it("projects page loads after recovery completes", async () => {
    await ProjectsPage.startPodmanButton.waitForDisplayed({
      reverse: true,
      timeout: PODMAN_RECOVERY,
      timeoutMsg:
        "Recovery screen did not clear after Start Podman. start_podman_machine may have failed — " +
        "check the activity center panel for a failed 'podman-startup' task.",
    });
    await ProjectsPage.createButton.waitForDisplayed({ timeout: UI_SETTLE });
  });

  // Step 6 — The decisive CLI assertion: the rootless socket answers, proving activate_podman_socket
  // launched `podman system service` under setsid (so it survived the SSH channel close) and
  // poll_ready reached a live socket — not just that `podman machine list` reported Running:true.
  it("podman-machine-default is reachable after recovery", async () => {
    const info = spawnSync(
      "podman",
      ["--connection", DEFAULT_MACHINE, "info", "--format", "json"],
      { encoding: "utf8", shell: false },
    );
    assert(
      info.status === 0,
      `podman --connection ${DEFAULT_MACHINE} info failed (exit ${info.status}): ${info.stderr.trim()}. ` +
        "The socket may not have been activated — check activate_podman_socket and poll_ready in backends/mod.rs.",
    );
  });

  // Step 7 — Sandbox operations target mio-tests (PODMAN_CONNECTION), independent of the
  // podman-machine-default probe, so creating a project must still succeed after recovery.
  it("can create a sandbox on mio-tests after Podman recovery", async () => {
    const name = `e2e-recovery-${Date.now()}`;
    try {
      const row = await ProjectsPage.createProject(name, { timeout: 600000 });
      await row.waitForDisplayed(COMMAND);
    } finally {
      // Best-effort cleanup of the container on mio-tests; the workspace dir is left for onComplete.
      podman(LOCAL_CONNECTION, ["rm", "-f", name], { check: false });
    }
  });
});
