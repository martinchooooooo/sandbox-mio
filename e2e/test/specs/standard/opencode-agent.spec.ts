import assert from "node:assert";
import fs from "fs";
import path from "path";

import ProjectsPage from "../../pageobjects/projects.page.js";
import ProjectRow from "../../pageobjects/project-row.component.js";
import workspaceDashboard from "../../pageobjects/workspace-dashboard.page.js";
import { podman, podmanOk, LOCAL_CONNECTION } from "../../../lib/podman-connection.js";

const WORKSPACE_ROOT = process.env.SANDBOX_WORKSPACE_ROOT;

// Must equal INFERENCE_VM_PORT in src/lib/agents/opencode.ts and src-tauri/src/sync/mod.rs. The
// constant is asserted in the written config below so a drift between the two languages is caught.
const INFERENCE_VM_PORT = 51199;
const EXPECTED_BASE_URL = `http://host.containers.internal:${INFERENCE_VM_PORT}/v1`;

// Invoke a Tauri command from inside the running webview. Mirrors the heavy local-llm spec; the
// command bridge is the only way to drive provision_opencode in isolation from the install flow.
async function invoke<T>(cmd: string, args: Record<string, unknown>): Promise<T> {
  return browser.execute(
    (c, a) =>
      // @ts-expect-error __TAURI_INTERNALS__ is injected into the webview at runtime.
      window.__TAURI_INTERNALS__.invoke(c, a),
    cmd,
    args,
  ) as Promise<T>;
}

// `function ()` (not an arrow) so `this.timeout` raises the suite ceiling: the OpenCode mise tool
// downloads a self-contained binary that can take a while on a cold run.
describe("OpenCode agent — install, provision, merge", function () {
  this.timeout(900000);

  const projectName = `e2e-opencode-${Date.now()}`;
  const row: ProjectRow = ProjectsPage.row(projectName);
  let workspaceDir: string;
  let manifestPath: string;
  let configPath: string;

  before(() => {
    expect(WORKSPACE_ROOT).toBeTruthy();
    workspaceDir = path.join(WORKSPACE_ROOT!, projectName);
    manifestPath = path.join(workspaceDir, "mise.toml");
    configPath = path.join(workspaceDir, "opencode.json");
  });

  after(() => {
    podman(LOCAL_CONNECTION, ["rm", "-f", projectName], { check: false });
  });

  // Safety net: if a test above throws while still attached to the dashboard webview, later
  // tests must not inherit that attachment — the global `before` hook only pins the session once.
  afterEach(async () => {
    await workspaceDashboard.switchToAppWebview().catch(() => {});
  });

  afterEach(async function () {
    if (this.currentTest?.state !== "failed") return;
    const diag = path.join(process.cwd(), ".diag");
    fs.mkdirSync(diag, { recursive: true });
    const stamp = (this.currentTest?.title ?? "test").replace(/[^a-z0-9]+/gi, "-").slice(0, 40);
    try {
      const url = await browser.getUrl();
      const src = await browser.getPageSource();
      fs.writeFileSync(path.join(diag, `${stamp}.html`), `<!-- url: ${url} -->\n${src}`);
      await browser.saveScreenshot(path.join(diag, `${stamp}.png`));
      console.log(`[diag] url=${url} — dumped ${stamp}.html/.png to ${diag}`);
    } catch (e) {
      console.log(`[diag] could not capture page state: ${String(e)}`);
    }
  });

  it("creates a project with an Agents section showing no agents", async () => {
    await ProjectsPage.open();
    await ProjectsPage.ensurePodmanReady();
    await ProjectsPage.createProject(projectName, { timeout: 600000 });

    await browser.waitUntil(() => fs.existsSync(manifestPath), {
      timeout: 60000,
      interval: 1000,
      timeoutMsg: `mise manifest ${manifestPath} did not appear after create`,
    });

    // Package/agent management now requires a workspace to be open (Resolved Decision 3,
    // docs/features/responsive-panel/02_dev.md) — there is no inline fallback in the row anymore.
    await row.openWorkspace();
    await workspaceDashboard.switchToDashboard(projectName);

    await workspaceDashboard.noAgentsText.waitForExist({
      timeout: 5000,
      timeoutMsg: "Agents section with 'No agents installed.' did not appear in the dashboard pane",
    });
  });

  it("installs OpenCode through the UI and provisions opencode.json", async () => {
    await workspaceDashboard.switchToDashboard(projectName);
    await workspaceDashboard.startAgentInstall("OpenCode");

    // The agent row appears once `mise use opencode` writes the tool to mise.toml; the binary is a
    // self-contained download (no npm postinstall), so this is lighter than the Claude Code install.
    await workspaceDashboard.waitForAgentRow("OpenCode", 900000);

    const manifest = fs.readFileSync(manifestPath, "utf8");
    assert(
      manifest.includes("opencode"),
      `mise.toml does not list "opencode" after agent install. Contents:\n${manifest}`,
    );

    // Decisive assertion: `opencode --version` proves the mise binary install produced a runnable
    // tool (unlike Claude Code, OpenCode needs no npm postinstall step).
    const ver = podman(LOCAL_CONNECTION, [
      "exec", "--workdir", "/root/work", projectName,
      "mise", "exec", "--", "opencode", "--version",
    ], { check: false });
    assert(
      /\d/.test(ver),
      `"opencode --version" printed "${ver}" with no version number. The mise "opencode" tool may ` +
        "not have installed a runnable binary.",
    );

    // Provisioning runs after install inside the same busy scope, so opencode.json is on disk by
    // the time the agent row renders. It is a host-side write into the bind-mounted workspace.
    await browser.waitUntil(() => fs.existsSync(configPath), {
      timeout: 30000,
      interval: 500,
      timeoutMsg: `opencode.json was not written to ${configPath} after install — provision_opencode ` +
        "did not run, or wrote to the wrong path.",
    });

    const config = JSON.parse(fs.readFileSync(configPath, "utf8"));
    assert.equal(
      config["$schema"],
      "https://opencode.ai/config.json",
      `opencode.json is missing the OpenCode $schema. Got: ${JSON.stringify(config["$schema"])}`,
    );
    const provider = config.provider?.["local-llm"];
    assert(provider, `opencode.json has no provider["local-llm"]. Full config:\n${JSON.stringify(config, null, 2)}`);
    assert.equal(
      provider.npm,
      "@ai-sdk/openai-compatible",
      `local-llm provider loader is "${provider.npm}", expected "@ai-sdk/openai-compatible".`,
    );
    assert.equal(
      provider.options?.baseURL,
      EXPECTED_BASE_URL,
      `local-llm baseURL is "${provider.options?.baseURL}", expected "${EXPECTED_BASE_URL}". A ` +
        "mismatch means INFERENCE_VM_PORT drifted between src/lib/agents/opencode.ts and the forward.",
    );
    assert(
      provider.options?.apiKey,
      "local-llm provider has no apiKey. @ai-sdk/openai-compatible always sends an Authorization " +
        "header; a missing dummy key makes OpenCode fail before reaching llama-server.",
    );
    assert(
      provider.models && Object.keys(provider.models).length > 0,
      `local-llm provider.models is empty. It must be generated from the model catalogue. Got: ` +
        JSON.stringify(provider.models),
    );
    assert.equal(
      config.model,
      "local-llm/phi-4-mini-reasoning",
      `default model is "${config.model}", expected "local-llm/phi-4-mini-reasoning".`,
    );
  });

  it("shows a 'start a model' hint while no inference server is running", async () => {
    // No inference server is started in this spec, so the provisioned-but-endpoint-down state must
    // render the /models hint rather than the provider-configured line or an error.
    await workspaceDashboard.switchToDashboard(projectName);
    const hint = await workspaceDashboard.agentProviderHint("OpenCode");
    await hint.waitForDisplayed({
      timeout: 10000,
      timeoutMsg: "OpenCode row did not show the 'Start a model' hint while inference is stopped.",
    });
    await expect(hint).toHaveText(/start a model/i);
  });

  it("re-provisioning merges into an existing config without clobbering user edits", async () => {
    // Seed a config carrying a user-chosen model and an unrelated provider, then re-run the exact
    // command the install path calls. The merge must keep both and never overwrite the model.
    const seeded = {
      model: "anthropic/claude-opus-4-1",
      provider: { anthropic: { npm: "@ai-sdk/anthropic", name: "Anthropic" } },
      theme: "tokyonight",
    };
    fs.writeFileSync(configPath, JSON.stringify(seeded, null, 2));

    await invoke<void>("provision_opencode", {
      workspacePath: workspaceDir,
      relativePath: "opencode.json",
      providerId: "local-llm",
      providerJson: {
        npm: "@ai-sdk/openai-compatible",
        name: "Local LLM",
        options: { baseURL: EXPECTED_BASE_URL, apiKey: "sk-local" },
        models: { "phi-4-mini-reasoning": { name: "Phi-4 mini reasoning" } },
      },
      defaultModel: "local-llm/phi-4-mini-reasoning",
    });

    const merged = JSON.parse(fs.readFileSync(configPath, "utf8"));
    assert.equal(
      merged.model,
      "anthropic/claude-opus-4-1",
      `merge overwrote the user's top-level model to "${merged.model}". It must be set only when absent.`,
    );
    assert(
      merged.provider?.anthropic,
      "merge dropped the user's existing anthropic provider. It must only set provider[local-llm].",
    );
    assert(
      merged.provider?.["local-llm"]?.options?.baseURL === EXPECTED_BASE_URL,
      "merge did not add the local-llm provider block.",
    );
    assert.equal(merged.theme, "tokyonight", "merge dropped an unrelated top-level key (theme).");
    assert.equal(
      merged["$schema"],
      "https://opencode.ai/config.json",
      "merge did not ensure the $schema key.",
    );
  });

  it("surfaces a parse error rather than clobbering a corrupt config", async () => {
    fs.writeFileSync(configPath, "{ this is not valid json");
    let threw = false;
    try {
      await invoke<void>("provision_opencode", {
        workspacePath: workspaceDir,
        relativePath: "opencode.json",
        providerId: "local-llm",
        providerJson: { npm: "@ai-sdk/openai-compatible" },
        defaultModel: null,
      });
    } catch {
      threw = true;
    }
    assert(threw, "provision_opencode silently succeeded on a corrupt opencode.json; it must error rather than overwrite.");
    assert.equal(
      fs.readFileSync(configPath, "utf8"),
      "{ this is not valid json",
      "provision_opencode overwrote a corrupt config instead of leaving it untouched.",
    );
  });

  it("destroy removes the container", async () => {
    await row.destroy();
    assert(
      !podmanOk(LOCAL_CONNECTION, ["inspect", projectName]),
      `Container "${projectName}" still exists after destroy.`,
    );
  });
});
