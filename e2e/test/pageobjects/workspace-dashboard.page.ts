import { type Element } from "./app.page.js";
import { AGENT_INSTALL, COMMAND, PACKAGE_INSTALL, PANEL_RENDER, SUBPROCESS } from "../../lib/timeouts.js";

// A DOM region of the single `main-navigation-panel` webview, not a separate one — no window
// switching needed (docs/features/consolidate-app-container/02_dev.md Phase C).
type DashboardSectionId = "overview" | "tools" | "ports" | "network" | "traffic" | "remote" | "danger";

// iron-proxy logs a CONNECT row's host as `host:port`; mitmproxy rows carry the bare host.
const stripPort = (host: string): string => host.replace(/:\d+$/, "");

class WorkspaceDashboardPage {
  // `[data-section-nav]` renders for every section once WorkspaceDashboard.svelte's `workspacePath`
  // resolves; the package input exists only in the Tools section. Scoped to `sandbox`'s dashboard so a
  // switch between two sandboxes cannot pass on the outgoing one.
  async waitForRendered(sandbox: string): Promise<void> {
    await $(`[data-dashboard='${sandbox}'] [data-section-nav]`).waitForExist({
      timeout: PANEL_RENDER,
      timeoutMsg: `dashboard content for "${sandbox}" never rendered (workspacePath likely never resolved)`,
    });
  }

  sectionItem(id: DashboardSectionId): Element {
    return $(`[data-section-nav-item="${id}"]`);
  }

  /** Switches to a section (a no-op when it is already active) and waits until it is active. */
  async openSection(id: DashboardSectionId): Promise<void> {
    const item = this.sectionItem(id);
    await item.waitForExist({ timeout: PANEL_RENDER, timeoutMsg: `section "${id}" is not in the section list` });
    if ((await item.getAttribute("data-state")) !== "active") {
      await item.scrollIntoView();
      await item.click();
    }
    await browser.waitUntil(async () => (await item.getAttribute("data-state")) === "active", {
      timeout: COMMAND,
      timeoutMsg: `section "${id}" did not become active`,
    });
  }

  // `data-dashboard` (WorkspaceDashboard.svelte's `<main>`, valued with the sandbox name) scopes the
  // getters below: the sandbox header above it also renders a StatusRow, so an unscoped `errorRow`
  // lookup could resolve to the header's instead of the Dashboard's.
  get root(): Element {
    return $("[data-dashboard]");
  }

  // ── App launcher (docs/features/workspace-app-tabs/02_dev.md Phase D) ──────

  // Scoped to `root`: once a pane exists, its sandbox header tab (SandboxHeader.svelte) carries the
  // same `data-app` value as the launcher.
  launcherButton(kind: "terminal" | "agentweb" | "agentchat"): Element {
    return this.root.$(`button[data-app="${kind}"]`);
  }

  async launchApp(kind: "terminal" | "agentweb" | "agentchat"): Promise<void> {
    await this.#clickButton(this.launcherButton(kind));
  }

  // ── Package management ─────────────────────────────────────────────────────

  get packageInput(): Element {
    return $("input[placeholder='package name']");
  }

  get installButton(): Element {
    return $("button=install");
  }

  async #clickButton(button: Element): Promise<void> {
    const el = await button;
    await el.scrollIntoView();
    await el.click();
  }

  // One click is not enough: a click landing before WebView2 has routed input focus reports success
  // yet leaves the element unfocused, so the `onfocus` handler that fetches suggestions never fires.
  async #focusInput(input: Element): Promise<void> {
    const el = await input;
    await el.scrollIntoView();

    await browser.waitUntil(
      async () => {
        await el.click();
        return browser.execute((node) => document.activeElement === node, el);
      },
      {
        timeout: COMMAND,
        interval: 300,
        timeoutMsg: "input never gained document focus after switching to the dashboard webview",
      },
    );
  }

  async installPackage(pkg: string, { timeout = PACKAGE_INSTALL }: { timeout?: number } = {}): Promise<void> {
    await this.#focusInput(this.packageInput);
    await (await this.packageInput).setValue(pkg);
    await this.#clickButton(this.installButton);
    await this.waitForPackage(pkg, timeout);
  }

  async installedPackages(): Promise<string[]> {
    // Scoped to the active section: other panels (network policy, flows) render monospace spans too.
    const chips = await $$('[data-slot="section-nav-content"][data-state="active"] span.font-mono');
    const names: string[] = [];
    for (const chip of chips) names.push(await chip.getText());
    return names;
  }

  async waitForPackage(pkg: string, timeoutMs = PACKAGE_INSTALL): Promise<void> {
    await browser.waitUntil(async () => (await this.installedPackages()).includes(pkg), {
      timeout: timeoutMs,
      interval: 2000,
      timeoutMsg: `package "${pkg}" did not appear in the dashboard within ${timeoutMs}ms`,
    });
  }

  // ── Package autocomplete ────────────────────────────────────────────────────
  // Selectors are body-rooted, not input-rooted: the dropdown is portalled to document.body.

  async typeInPackageInput(text: string): Promise<void> {
    await this.#focusInput(this.packageInput);
    await (await this.packageInput).setValue(text);
  }

  async visibleSuggestions(): Promise<string[]> {
    const items = await $$(".fixed.bg-popover button[type='button']");
    const names: string[] = [];
    for (const item of items) {
      const span = await item.$("span.font-mono");
      if (await span.isExisting()) names.push(await span.getText());
    }
    return names;
  }

  async suggestionDescription(shortName: string): Promise<string> {
    const items = await $$(".fixed.bg-popover button[type='button']");
    for (const item of items) {
      const nameSpan = await item.$("span.font-mono");
      if (!(await nameSpan.isExisting())) continue;
      if ((await nameSpan.getText()) !== shortName) continue;
      const desc = await item.$("span.text-muted-foreground");
      return (await desc.isExisting()) ? desc.getText() : "";
    }
    return "";
  }

  async selectSuggestion(shortName: string): Promise<void> {
    const items = await $$(".fixed.bg-popover button[type='button']");
    for (const item of items) {
      const span = await item.$("span.font-mono");
      if ((await span.isExisting()) && (await span.getText()) === shortName) {
        await item.scrollIntoView();
        await item.click();
        return;
      }
    }
    throw new Error(`suggestion "${shortName}" not found in the dropdown`);
  }

  async packageInputValue(): Promise<string> {
    return (await this.packageInput).getValue();
  }

  // ── Agent management ────────────────────────────────────────────────────────

  get addAgentButton(): Element {
    return $("button=Add agent");
  }

  get noAgentsText(): Element {
    return $("span=No agents installed.");
  }

  agentInstallButton(agentLabel: string): Element {
    return $(`span*=${agentLabel}`).$("../..").$("button=install");
  }

  agentAuthButton(agentLabel: string): Element {
    return $(`span=${agentLabel}`).$("..").$("button*=thenticate");
  }

  agentStatusText(agentLabel: string): Element {
    return $(`span=${agentLabel}`).$("..").$("span.text-muted-foreground, span.text-amber-600");
  }

  agentProviderHint(agentLabel: string): Element {
    return $(`span=${agentLabel}`).$("..").$("a[href='/models']");
  }

  async startAgentInstall(agentLabel: string): Promise<void> {
    await this.#clickButton(this.addAgentButton);
    await this.#clickButton(this.agentInstallButton(agentLabel));
  }

  async waitForAgentRow(agentLabel: string, timeoutMs = AGENT_INSTALL): Promise<void> {
    await browser.waitUntil(async () => (await $(`span=${agentLabel}`)).isExisting(), {
      timeout: timeoutMs,
      interval: 3000,
      timeoutMsg: `agent row for "${agentLabel}" did not appear`,
    });
  }

  async waitForAgentStatus(agentLabel: string, text: string, timeoutMs = COMMAND): Promise<void> {
    await browser.waitUntil(
      async () => {
        try {
          return (await this.agentStatusText(agentLabel).getText()) === text;
        } catch {
          return false;
        }
      },
      { timeout: timeoutMs, interval: 500, timeoutMsg: `agent status for "${agentLabel}" did not become "${text}"` },
    );
  }

  // ── Port management (PortsManager.svelte) ──────────────────────────────────
  // `kindLabel` keys a row: "Exposed :4000" for a manually-exposed port, or the `.mio/ports.json`
  // label (e.g. "webapp") for a file-declared one.

  get exposePortInput(): Element {
    return $("input[placeholder='Expose a port (e.g. 3000)']");
  }

  get exposeButton(): Element {
    return $("button=Expose");
  }

  get noPortsText(): Element {
    return $("span=No ports open.");
  }

  portRow(kindLabel: string): Element {
    return $(`span=${kindLabel}`).$("..");
  }

  // portRow() chains `.$("..")` onto the label span, which throws when missing instead of
  // returning a not-existing element — that would abort waitUntil's poll instead of retrying.
  #portLabelSpan(kindLabel: string): Element {
    return $(`span=${kindLabel}`);
  }

  async exposePort(port: number): Promise<void> {
    const input = await this.exposePortInput;
    await input.scrollIntoView();
    await input.setValue(String(port));
    await this.#clickButton(this.exposeButton);
  }

  async waitForPortRow(kindLabel: string, timeoutMs = SUBPROCESS): Promise<void> {
    await browser.waitUntil(async () => (await this.#portLabelSpan(kindLabel)).isExisting(), {
      timeout: timeoutMs,
      interval: 500,
      timeoutMsg: `port row "${kindLabel}" did not appear`,
    });
  }

  async waitForPortRowGone(kindLabel: string, timeoutMs = SUBPROCESS): Promise<void> {
    await browser.waitUntil(async () => !(await this.#portLabelSpan(kindLabel).isExisting()), {
      timeout: timeoutMs,
      interval: 500,
      timeoutMsg: `port row "${kindLabel}" is still displayed`,
    });
  }

  async portRowHostPort(kindLabel: string): Promise<number> {
    const row = await this.portRow(kindLabel);
    const link = await row.$("button[data-port-url]");
    const text = await link.getText();
    const match = text.match(/:(\d+)\/?$/);
    if (!match) throw new Error(`port row "${kindLabel}" button text "${text}" has no port`);
    return Number(match[1]);
  }

  /** Only manually-exposed rows carry one; file-declared ports cannot be closed from the UI. */
  async hasCloseButton(kindLabel: string): Promise<boolean> {
    const row = await this.portRow(kindLabel);
    return (await row.$("button[title='Stop exposing']")).isExisting();
  }

  async closePortRow(kindLabel: string): Promise<void> {
    const row = await this.portRow(kindLabel);
    await this.#clickButton(row.$("button[title='Stop exposing']"));
  }

  // ── SSH access (SshAccessCard.svelte, in the Ports section) ─────────────────
  // Selected by data attributes, never by text: the panel's wording may change
  // (docs/features/host-tool-ssh-access/02_dev.md, Phase 0) and the Paraglide pass localises it.

  get sshAccess(): Element {
    return this.root.$("[data-ssh-access]");
  }

  get sshRefreshButton(): Element {
    return this.sshAccess.$("[data-ssh-refresh]");
  }

  /** The alias the panel hands to a desktop client; waits for the first fetch to land. */
  async sshAlias(): Promise<string> {
    const alias = await this.sshAccess.$("[data-ssh-alias]");
    await alias.waitForExist({ timeout: SUBPROCESS, timeoutMsg: "the SSH access panel never showed an alias" });
    return alias.getText();
  }

  /** `data-ssh-state`: "loading", "running", "stopped" or "deployed". */
  async waitForSshState(state: string, timeoutMs = SUBPROCESS): Promise<void> {
    await browser.waitUntil(async () => (await this.sshAccess.getAttribute("data-ssh-state")) === state, {
      timeout: timeoutMs,
      timeoutMsg: `the SSH access panel never reached state "${state}"`,
    });
  }

  async sshStateText(): Promise<string> {
    return this.sshAccess.$("[data-ssh-state-text]").getText();
  }

  /** A value from the panel's collapsed Details: "identity", "known-hosts" or "proxy-command". */
  async sshDetail(key: "identity" | "known-hosts" | "proxy-command"): Promise<string> {
    const value = await this.sshAccess.$(`[data-ssh-value='${key}']`);
    if (!(await value.isExisting())) {
      await this.#clickButton(this.sshAccess.$("[data-ssh-details] [data-slot='collapsible-trigger']"));
      await value.waitForExist({ timeout: COMMAND, timeoutMsg: "the SSH access Details never opened" });
    }
    // `title` holds the untruncated value; the text node is ellipsised in a narrow window.
    return (await value.getAttribute("title")) ?? "";
  }

  // ── Destroy (WorkspaceDashboard.svelte) ─────────────────────────────────────

  // The "Delete sandbox" section's button; its label is `projects_destroy` in messages/en.json.
  get destroyButton(): Element {
    return this.root.$("button=delete");
  }

  // ── Remote management (RemoteManager.svelte) ────────────────────────────────

  get deployButton(): Element {
    return this.root.$("button=deploy");
  }

  get deployConfirmButton(): Element {
    return this.root.$("button=Deploy");
  }

  get undeployButton(): Element {
    return this.root.$("button=undeploy");
  }

  // ── Network section (docs/features/podman-egress-boundary) ───────────────────
  // Selected by data attributes, never by placeholder text: the Paraglide pass localises placeholders.
  get netPolicySave(): Element {
    return $("[data-net-policy-save]");
  }

  get netPolicyApplyReport(): Element {
    return $("[data-net-policy-apply-report]");
  }

  get netPolicyDrift(): Element {
    return $("[data-net-policy-drift]");
  }

  async #typeInto(input: Element, text: string): Promise<void> {
    await input.waitForEnabled({ timeout: COMMAND });
    await input.setValue(text);
  }

  /** Adds an allow rule for `host` (the action toggle defaults to allow) without saving. */
  async addAllowRule(host: string): Promise<void> {
    await this.#typeInto($("[data-net-policy-target]"), host);
    await $("[data-net-policy-add-rule]").click();
  }

  /** Saves the policy; in the sidecar dialect this also applies it to the running sidecar. */
  async saveNetPolicy(): Promise<void> {
    await this.netPolicySave.waitForEnabled({ timeout: COMMAND });
    await this.netPolicySave.click();
    await this.netPolicyApplyReport.waitForExist({
      timeout: SUBPROCESS,
      timeoutMsg: "no apply report appeared after saving the net policy",
    });
  }

  secretRow(envVar: string): Element {
    return $(`[data-net-policy-secret='${envVar}']`);
  }

  /** Waits until a secret save or removal settles, failing with the editor's error if one appears. */
  async #awaitSecretCommit(done: () => Promise<boolean>, what: string): Promise<void> {
    const error = $("[data-net-policy-error]");
    await browser.waitUntil(async () => (await error.isExisting()) || (await done()), {
      timeout: SUBPROCESS,
      timeoutMsg: `${what} never settled`,
    });
    if (await error.isExisting()) throw new Error(`${what} failed: ${await error.getText()}`);
  }

  /** Adds a secret from the add form; this saves the whole policy and applies it on the sidecar. */
  async addSecret(envVar: string, hosts: string, value: string): Promise<void> {
    const form = $("[data-secret-form='add']");
    await this.#typeInto(form.$("[data-secret-env-var]"), envVar);
    await this.#typeInto(form.$("[data-secret-hosts]"), hosts);
    await this.#typeInto(form.$("[data-secret-value]"), value);
    await form.$("[data-secret-save]").click();
    // The row renders a tick before the form resets, so wait for both.
    await this.#awaitSecretCommit(
      async () => (await this.secretRow(envVar).isExisting()) && (await form.$("[data-secret-env-var]").getValue()) === "",
      `adding $${envVar}`,
    );
  }

  /** Edits a secret's hosts and/or replaces its value; an omitted field keeps its current content. */
  async editSecret(envVar: string, change: { hosts?: string; value?: string }): Promise<void> {
    await this.#clickButton(this.secretRow(envVar).$("[data-secret-edit]"));
    const form = $("[data-secret-form='edit']");
    await form.waitForExist({ timeout: COMMAND });
    if (change.hosts !== undefined) await this.#typeInto(form.$("[data-secret-hosts]"), change.hosts);
    if (change.value !== undefined) await this.#typeInto(form.$("[data-secret-value]"), change.value);
    await form.$("[data-secret-save]").click();
    await this.#awaitSecretCommit(async () => !(await form.isExisting()), `editing $${envVar}`);
  }

  async removeSecret(envVar: string): Promise<void> {
    await this.#clickButton(this.secretRow(envVar).$("[data-secret-remove]"));
    await this.#awaitSecretCommit(async () => !(await this.secretRow(envVar).isExisting()), `removing $${envVar}`);
  }

  async applyReportText(): Promise<string> {
    return this.netPolicyApplyReport.getText();
  }

  // ── Traffic section ─────────────────────────────────────────────────────────
  /** Hosts of every visible traffic row, in display order. */
  async flowHosts(): Promise<string[]> {
    const spans = await $$("[data-flow-row] [data-flow-host]");
    const hosts: string[] = [];
    for (const span of spans) hosts.push(stripPort(await span.getText()));
    return hosts;
  }

  /** Hosts of rows carrying a "blocked by" badge. */
  async rejectedFlowHosts(): Promise<string[]> {
    const rows = await $$("[data-flow-row]");
    const hosts: string[] = [];
    for (const row of rows) {
      if (await row.$("[data-flow-rejected-by]").isExisting()) hosts.push(stripPort(await row.$("[data-flow-host]").getText()));
    }
    return hosts;
  }

  async waitForFlowHost(host: string, { rejected = false, timeout = COMMAND } = {}): Promise<void> {
    await browser.waitUntil(
      async () => ((rejected ? await this.rejectedFlowHosts() : await this.flowHosts()).includes(host)),
      { timeout, timeoutMsg: `no ${rejected ? "rejected " : ""}traffic row for ${host}` },
    );
  }

  get hostSelect(): Element {
    return this.root.$("select");
  }

  get syncSwitch(): Element {
    return this.root.$("button[role='switch']");
  }

  /**
   * StatusRow's error line for the Dashboard's shared action runner — deploy/undeploy/destroy/sync
   * all run through it (`action` in WorkspaceDashboard.svelte), so one selector covers all of them,
   * scoped to `root` (see the comment on it above).
   */
  get errorRow(): Element {
    return this.root.$("div[role='status'].text-destructive");
  }
}

export default new WorkspaceDashboardPage();
