import assert from "node:assert";
import { spawnSync } from "child_process";

import ProjectsPage from "../../pageobjects/projects.page.js";
import ActivityCenter from "../../pageobjects/activity.page.js";
import { podman, LOCAL_CONNECTION } from "../../../lib/podman-connection.js";
import { activatePodmanSocket, DEFAULT_MACHINE } from "../../../lib/podman-machine.js";
import { COMMAND, PANEL_RENDER, PODMAN_RECOVERY, SUBPROCESS, UI_SETTLE } from "../../../lib/timeouts.js";
import { captureDiagOnFailure } from "../../../lib/diag.js";

// Proves the Podman startup-recovery feature end to end (docs/features/podman-startup-recovery):
// with podman-machine-default stopped, /projects shows the recovery screen; clicking "Start Podman"
// runs the real check_podman_ready/start_podman_machine commands (podman machine start + the setsid
// socket-activation over `podman machine ssh` + poll_ready) and the page recovers. The decisive
// proof is Step 6: `podman --connection podman-machine-default info` answering proves the rootless
// socket was actually activated, not merely that the machine reports Running:true.
//
// The default suite spec (not heavy/): it downloads no artefacts but takes 30–60 s for the
// stop/start cycle, so the suite timeout is widened to 120 s.
//
// Topology mirrors production: sandboxes target mio-tests (the PODMAN_CONNECTION set in
// beforeSession), while the startup probe targets podman-machine-default. Because mio-tests stays
// running throughout, check_other_machines_running() returns true and the `wsl --shutdown` guard
// correctly skips the reset — that path is out of scope here (see the e2e plan's Environment Notes).
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
    // Restarting is not enough to undo this spec: check_podman_ready also runs poll_ready →
    // `podman --connection <machine> info` (src-tauri/src/machine/mod.rs), so a machine that is
    // Running with a dead rootless socket still fails the probe. A mid-spec failure can leave
    // exactly that state, and this spec sits twelfth of fifteen in the alphabetical standard order,
    // so the specs after it would each hit the "Start Podman" screen — one failure becoming four.
    // Both steps are best-effort: warn rather than throw, so cleanup never masks a real failure.
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

  // Step 1 — Land on home so the projects onMount fires fresh on the next step. onMount only runs on
  // route entry, so revisiting /projects (a prior spec may have left us there) would not re-probe.
  it("navigates home so the projects onMount fires fresh on the next step", async () => {
    await ProjectsPage.goHome();
    const homeHeading = $("h1");
    await homeHeading.waitForDisplayed({ timeout: UI_SETTLE });
  });

  // Step 2 — Entering /projects probes check_podman_ready, which detects the stopped machine and
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

  // Step 3 — Clicking Start Podman puts the button into its loading/disabled state for the duration
  // of start_podman_machine (loading={busy === "start"}, disabled={busy !== null}).
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

  // Step 5 — Recovery completes: startPodman() set podmanUnavailable = false, removing the recovery
  // screen. The disappearance of the Start Podman button is the discriminating signal (the Create
  // form above the list stays mounted throughout, so Create appearing does not prove recovery).
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
