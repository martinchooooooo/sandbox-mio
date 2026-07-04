import assert from "node:assert";
import fs from "fs";
import path from "path";

import ProjectsPage from "../../pageobjects/projects.page.js";
import ProjectRow from "../../pageobjects/project-row.component.js";
import ModelsPage from "../../pageobjects/models.page.js";
import { podman, podmanOk, LOCAL_CONNECTION } from "../../../lib/podman-connection.js";
import { captureDiagOnFailure } from "../../../lib/diag.js";

// HEAVY spec: realizes the dev plan's "assert the container reaches host.containers.internal:51199"
// requirement (docs/features/opencode-agent/02_dev.md, Cross-Cutting Concerns). It is the only test
// that exercises the full app-wide reverse-forward bridge: a running sandbox container on the
// mio-tests machine reaching the host inference server through the fixed VM port. Excluded from the
// default glob (downloads + loads a real GGUF and starts a container); run via `pnpm run test:heavy`.

const WORKSPACE_ROOT = process.env.SANDBOX_WORKSPACE_ROOT;
const INFERENCE_VM_PORT = 51199; // must equal sync::INFERENCE_VM_PORT and opencode.ts INFERENCE_VM_PORT

const APP_DATA_MODELS = path.join(
  process.env.APPDATA ?? path.join(process.env.USERPROFILE ?? "", "AppData", "Roaming"),
  "com.tincho.sandbox-mio",
  "models",
);

// A raw HTTP/1.0 GET over bash's /dev/tcp, so no curl/wget needs to exist in the base image. Prints
// the full response (status line + body) on success; exits non-zero (printing nothing) when the
// listener is absent, which is exactly the post-stop state the negative assertion needs.
function probeFromContainer(container: string): string {
  const script =
    `exec 3<>/dev/tcp/host.containers.internal/${INFERENCE_VM_PORT} && ` +
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

    // host.containers.internal must resolve in the container netns, or the probe can never connect
    // regardless of the forward. Assert the precondition up front for a clear failure.
    const hosts = podman(LOCAL_CONNECTION, ["exec", projectName, "cat", "/etc/hosts"], { check: false });
    assert(
      /host\.containers\.internal/.test(hosts),
      `host.containers.internal is not in the container's /etc/hosts, so the OpenCode baseURL can ` +
        `never resolve. /etc/hosts:\n${hosts}`,
    );

    // Before inference starts there is no forward — the listener must be absent.
    const before = probeFromContainer(projectName);
    assert(
      !/HTTP\/1\./.test(before),
      `Container reached :${INFERENCE_VM_PORT} before inference started. No forward should exist ` +
        `yet. Response:\n${before}`,
    );

    // ── Start inference, which creates the app-wide reverse forward ──────────────────────────
    await navigateHome();
    await ModelsPage.open();
    await ModelsPage.startDownload(testModelId);
    await ModelsPage.waitForDownloaded(testModelId, 480000);
    await ModelsPage.startInference(testModelId);
    await ModelsPage.waitForRunning(180000);

    // The reverse forward is created right after the server is Ready. Give Mutagen a moment to
    // bring the session up, then assert the container reaches the host server through the VM port.
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
          `Container never got HTTP 200 from host.containers.internal:${INFERENCE_VM_PORT}/v1/models ` +
          `while inference was running. The reverse forward was not created, or bound the VM's ` +
          `127.0.0.1 (invisible to containers) instead of 0.0.0.0. Last response:\n${response}`,
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
          `Container still reached :${INFERENCE_VM_PORT} after stop_inference. The reverse forward ` +
          `was orphaned — it must be terminated in stop_inference. Last response:\n${after}`,
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
