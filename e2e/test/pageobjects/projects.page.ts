import AppPage, { showHome, type Element } from "./app.page.js";
import ProjectRow from "./project-row.component.js";
import { COMMAND, PODMAN_RECOVERY, SUBPROCESS, UI_SETTLE } from "../../lib/timeouts.js";

// Home (src/routes/projects/+page.svelte). Every action here calls showHome() first: the create card
// and the sandbox cards are unmounted while a sandbox is in view.
class ProjectsPage extends AppPage {
  get root(): Element {
    return $("main[data-home]");
  }

  // docs/features/sandbox-list-create
  get createRow(): Element {
    return $("li[data-create-row]");
  }

  get nameInput(): Element {
    return this.createRow.$("input[placeholder='my-project']");
  }

  get createButton(): Element {
    return this.createRow.$("button=Create");
  }

  get useExistingFolderLink(): Element {
    return this.createRow.$("button*=existing folder");
  }

  // Never click this: it spawns a real, blocking OS dialog. Specs assert its presence or absence to
  // confirm the mode toggle (docs/features/sandbox-list-create/03_e2e.md).
  get chooseFolderButton(): Element {
    return this.createRow.$("button*=Choose folder");
  }

  get backToNameButton(): Element {
    return this.createRow.$("[aria-label='Back']");
  }

  async useExistingFolderMode(): Promise<void> {
    await (await this.useExistingFolderLink).click();
  }

  // docs/features/microsandbox-provider — the create row's provider picker. Keyed on the attribute
  // rather than the label, which is still a hard-coded English literal pending the i18n pass.
  providerOption(provider: string): Element {
    return this.createRow.$(`button[data-provider-option='${provider}']`);
  }

  // The picker sits behind the shared AdvancedSection disclosure, closed by default on every page
  // visit (docs/features/desktop-app-feel/02_dev.md Phase C) — this is its trigger.
  get advancedTrigger(): Element {
    return this.createRow.$("button=Advanced");
  }

  /** Select which provider the next `createProject()` creates on. */
  async chooseProvider(provider: string): Promise<void> {
    await showHome();
    const opt = await this.providerOption(provider);
    const visible = (await opt.isExisting()) && (await opt.isDisplayed());
    if (!visible) {
      const trigger = await this.advancedTrigger;
      await trigger.waitForClickable({ timeout: UI_SETTLE });
      await trigger.click();
    }
    const btn = await this.providerOption(provider);
    await btn.waitForClickable({ timeout: UI_SETTLE });
    await btn.click();
  }

  // docs/features/microsandbox-network-policy — the egress choice, shown only for a provider whose
  // ProviderCapabilities.egressPolicy is true. Call after chooseProvider(), which opens Advanced.
  egressOption(mode: string): Element {
    return this.createRow.$(`button[data-egress-option='${mode}']`);
  }

  /** Select the network mode the next `createProject()` creates with. */
  async chooseEgress(mode: "open" | "allowlist"): Promise<void> {
    const btn = await this.egressOption(mode);
    await btn.waitForClickable({ timeout: UI_SETTLE });
    await btn.click();
  }

  // ── Podman startup-recovery gate ─────────────────────────────────────────────
  // This button's presence is the gate signal, not the Create button: the create form stays
  // mounted while the recovery screen replaces the sandbox list.
  get startPodmanButton(): Element {
    return $("button=Start Podman");
  }

  get podmanNotRunningText(): Element {
    return $("p=Podman machine is not running");
  }

  /** A no-op while Home already shows, so Home is not remounted and does not re-probe Podman. */
  async open(timeout = COMMAND): Promise<void> {
    await showHome(timeout);
  }

  /**
   * Call between open() and createProject(). Drives the app's own recovery rather than
   * `podman machine start`, which can leave the rootless socket down. No-op when already ready.
   */
  async ensurePodmanReady(probeWindow = COMMAND, recoverTimeout = PODMAN_RECOVERY): Promise<void> {
    const btn = await this.startPodmanButton;
    try {
      await btn.waitForDisplayed({ timeout: probeWindow });
    } catch {
      return; // gate never appeared within the probe window → Podman is ready
    }
    await btn.click();
    await btn.waitForDisplayed({
      reverse: true,
      timeout: recoverTimeout,
      timeoutMsg:
        "Podman recovery did not clear the 'Start Podman' gate; podman-machine-default failed to " +
        "start or its rootless socket never came up (see start_podman_machine / activate_podman_socket " +
        "in src-tauri, and the activity center for a failed 'podman-startup' task).",
    });
    await this.createButton.waitForDisplayed({ timeout: UI_SETTLE });
  }

  // Default budget is SUBPROCESS, not COMMAND: create_sandbox chains real podman lifecycle calls
  // (create_instance, SSH identity/key install) that measured ~5.5s combined even on an idle
  // machine with no concurrent load — COMMAND's 10s left near-zero margin (docs/features/lib-decomposition/04_improvements.md).
  // Widen further for the first sandbox on a connection: it triggers the one-time base-image
  // build, so the row can take minutes to appear.
  async createProject(name: string, { timeout = SUBPROCESS }: { timeout?: number } = {}): Promise<ProjectRow> {
    await showHome();
    await (await this.nameInput).setValue(name);
    const btn = await this.createButton;
    await btn.waitForClickable({ timeout: UI_SETTLE });
    await btn.click();

    const row = this.row(name);
    await row.waitForDisplayed(timeout);
    return row;
  }

  /** Does not assert the row exists yet. */
  row(name: string): ProjectRow {
    return new ProjectRow(name);
  }
}

export default new ProjectsPage();
