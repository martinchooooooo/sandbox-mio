import assert from "node:assert";
import fs from "fs";
import path from "path";

import ProjectsPage from "../../pageobjects/projects.page.js";
import ProjectRow from "../../pageobjects/project-row.component.js";
import ModelsPage from "../../pageobjects/models.page.js";
import { podman, podmanOk, LOCAL_CONNECTION } from "../../../lib/podman-connection.js";
import { captureDiagOnFailure } from "../../../lib/diag.js";
import { APP_DATA_ROOT } from "../../../lib/run-identity.js";

// HEAVY (downloads a GGUF): the only end-to-end proof that a sandbox reaches the host inference
// server on its own loopback (docs/features/local-llm-reachability/02_dev.md).

const WORKSPACE_ROOT = process.env.SANDBOX_WORKSPACE_ROOT;
// Must equal INFERENCE_GUEST_PORT in src-tauri/src/inference/mod.rs and src/lib/agents/local-llm.ts.
const INFERENCE_GUEST_PORT = 51199;

// The app writes models under the slug-keyed app-data root the launcher sets via MIO_APP_DATA_DIR
// (src-tauri/src/paths.rs), not under %APPDATA%/<bundle identifier>.
const APP_DATA_MODELS = path.join(APP_DATA_ROOT, "models");

// A raw HTTP/1.0 GET over bash's /dev/tcp, so no curl/wget needs to exist in the base image. Prints
// the full response (status line + body) on success; exits non-zero (printing nothing) when the
// listener is absent, which is exactly the post-stop state the negative assertion needs.
function probeFromContainer(container: string): string {
  const script =
    `exec 3<>/dev/tcp/127.0.0.1/${INFERENCE_GUEST_PORT} && ` +
    `printf 'GET /v1/models HTTP/1.0\\r\\nHost: h\\r\\n\\r\\n' >&3 && cat <&3`;
  return podman(LOCAL_CONNECTION, ["exec", container, "bash", "-c", script], { check: false });
}

async function navigateHome(): Promise<void> {
  const origin = new URL(await browser.getUrl()).origin;
  await browser.url(`${origin}/`);
}

describe("OpenCode forward — container reaches host inference", function () {
  this.timeout(900000);

  const projectName = `e2e-ocfwd-${Date.now()}`;
  const row: ProjectRow = ProjectsPage.row(projectName);
  let testModelId: string;

  before(() => {
    expect(WORKSPACE_ROOT).toBeTruthy();
    const raw = process.env.MIO_TEST_MODEL;
    assert(raw, "MIO_TEST_MODEL is not set; the forward test needs the tiny test GGUF descriptor.");
    testModelId = (JSON.parse(raw) as { id: string }).id;
  });

  after(async () => {
    // Best-effort: stop the server (also tears down the forward) and remove the container + GGUF.
    try {
      await browser.execute(() => {
        // @ts-expect-error __TAURI_INTERNALS__ is injected into the webview at runtime.
        return window.__TAURI_INTERNALS__?.invoke("stop_inference");
      });
    } catch {
      /* best-effort */
    }
    podman(LOCAL_CONNECTION, ["rm", "-f", projectName], { check: false });
    fs.rmSync(path.join(APP_DATA_MODELS, testModelId), { recursive: true, force: true });
  });

  captureDiagOnFailure();

  it("bridges the container to the host inference server while a model runs, and tears down on stop", async () => {
    // ── A running sandbox container, the source of the in-container probe ─────────────────────
    await ProjectsPage.open();
    await ProjectsPage.ensurePodmanReady();
    await ProjectsPage.createProject(projectName, { timeout: 600000 });
    // A freshly created project is not auto-started (see project-start-stop.spec.ts); the in-
    // container probe needs a running container to `podman exec` into.
    await row.start();
    await row.waitForRunning(30000);

    // Before inference starts there is no forward — the listener must be absent.
    const before = probeFromContainer(projectName);
    assert(
      !/HTTP\/1\./.test(before),
      `Container reached :${INFERENCE_GUEST_PORT} before inference started. No forward should exist ` +
        `yet. Response:\n${before}`,
    );

    // ── Start inference, which creates the sandbox's inference forward ────────────────────────
    await navigateHome();
    await ModelsPage.open();
    await ModelsPage.startDownload(testModelId);
    await ModelsPage.waitForDownloaded(testModelId, 480000);
    await ModelsPage.startInference(testModelId);
    await ModelsPage.waitForRunning(180000);

    // The inference forward is created right after the server is Ready. Give Mutagen a moment to
    // bring the session up, then assert the container reaches the host server on its own loopback.
    let response = "";
    await browser.waitUntil(
      () => {
        response = probeFromContainer(projectName);
        return /HTTP\/1\.[01] 200/.test(response);
      },
      {
        timeout: 30000,
        interval: 2000,
        timeoutMsg:
          `Container never got HTTP 200 from 127.0.0.1:${INFERENCE_GUEST_PORT}/v1/models while ` +
          `inference was running. The sandbox's inference-<sandbox> forward was not created; check ` +
          `the "inference" target in mio.log. Last response:\n${response}`,
      },
    );
    assert(
      /"data"|"object"/.test(response),
      `/v1/models answered 200 but the body is not the OpenAI model-list JSON. Body:\n${response}`,
    );

    // ── Stop inference — the forward must be torn down so the port stops answering ────────────
    await ModelsPage.stopInference();
    let after = "";
    await browser.waitUntil(
      () => {
        after = probeFromContainer(projectName);
        return !/HTTP\/1\./.test(after);
      },
      {
        timeout: 30000,
        interval: 2000,
        timeoutMsg:
          `Container still reached :${INFERENCE_GUEST_PORT} after stop_inference. The inference ` +
          `forward was orphaned — stop_inference must close it. Last response:\n${after}`,
      },
    );
  });

  it("destroy removes the container", async () => {
    await navigateHome();
    await ProjectsPage.open();
    await row.waitForDisplayed();
    await row.destroy();
    assert(
      !podmanOk(LOCAL_CONNECTION, ["inspect", projectName]),
      `Container "${projectName}" still exists after destroy.`,
    );
  });
});
