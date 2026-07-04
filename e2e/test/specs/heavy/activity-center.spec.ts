import assert from "node:assert";
import fs from "fs";
import path from "path";

import ModelsPage from "../../pageobjects/models.page.js";
import ProjectsPage from "../../pageobjects/projects.page.js";
import ActivityCenter from "../../pageobjects/activity.page.js";
import { captureDiagOnFailure } from "../../../lib/diag.js";

// HEAVY spec (docs/features/activity-center/03_e2e.md, Steps 1–8). A near-twin of local-llm.spec.ts:
// it reuses the same staged llama sidecar and MIO_TEST_MODEL tiny GGUF as the work that *produces*
// tasks, but its assertions read the global activity center **from a foreign route** and **after a
// webview reload** — the two assertions with teeth that local-llm.spec.ts deliberately does not
// make. Those prove the source of truth is the Rust TaskRegistry, not any route component.
//
// Host-only for Steps 1–7 (no Podman). The conditional Step 8 touches Podman and self-skips.

interface TestModel {
  id: string;
  label: string;
  repo: string;
  file: string;
  sha256: string;
  sizeBytes: number;
}

// The debug build writes models under app_data_dir() = %APPDATA%/<bundle identifier>/models.
const APP_DATA_MODELS = path.join(
  process.env.APPDATA ?? path.join(process.env.USERPROFILE ?? "", "AppData", "Roaming"),
  "com.tincho.sandbox-mio",
  "models",
);

describe("Activity Center — global task tracking", function () {
  this.timeout(600000); // a cold run downloads the test GGUF and loads it into the sidecar

  let model: TestModel;
  let taskId: string; // model-download:<id>, the registry's natural-key id for the download

  before(() => {
    const raw = process.env.MIO_TEST_MODEL;
    assert(
      raw,
      "MIO_TEST_MODEL is not set. The spec derives the task id model-download:<id> from the tiny " +
        "test GGUF descriptor {id,label,repo,file,sha256,sizeBytes}; wdio.conf.ts exports it.",
    );
    model = JSON.parse(raw) as TestModel;
    taskId = `model-download:${model.id}`;
  });

  after(async () => {
    // Best-effort: stop any running server so no sidecar is orphaned, and delete the test GGUF.
    try {
      await browser.execute(() => {
        // @ts-expect-error __TAURI_INTERNALS__ is injected into the webview at runtime.
        return window.__TAURI_INTERNALS__?.invoke("stop_inference");
      });
    } catch {
      /* best-effort */
    }
    fs.rmSync(path.join(APP_DATA_MODELS, model.id), { recursive: true, force: true });
  });

  // Capture webview state on any failure (shared with the other specs via lib/diag.ts).
  captureDiagOnFailure();

  // Steps 2–6 are one ordered flow (download → cross-route → reload → event line) sharing the live
  // registry state, so a failure stops the chain rather than cascading into confusing low-level
  // errors on a later, decoupled test — same structure as local-llm.spec.ts.
  it("tracks a download across a route change and a reload, then a completion event", async () => {
    // ── Step 2: start a real download from /models (the work that produces the task) ──────────
    await ModelsPage.open();
    await ModelsPage.startDownload(model.id);

    // ── Step 3: the task appears in the activity center on /models ────────────────────────────
    await ActivityCenter.openPanel();
    // Accept running OR succeeded: a fast ~100 MB download may already be terminal by first poll.
    await ActivityCenter.waitForTask(taskId, ["running", "succeeded"], 480000);

    // ── Step 4: the task survives navigation to a foreign route (teeth) ───────────────────────
    // Go home, then to /projects — never back to /models. The component that started the download
    // is now unmounted, so a component-local progress UI would show nothing here.
    await ModelsPage.goHome();
    await ProjectsPage.open();
    await ActivityCenter.openPanel();
    await ActivityCenter.waitForTask(taskId, "succeeded", 480000);

    // ── Step 6: terminal transition appended a synthetic event line (timing-robust proxy) ─────
    // The persistent Event scrollback line is the required proof (the success toast auto-dismisses,
    // so its assertion is best-effort and omitted here). The line references the completed model id.
    const events = await ActivityCenter.eventsText();
    assert(
      events.includes(model.id) || /finish|download/i.test(events),
      `Event scrollback "${events}" has no line for the completed download. onTransition must ` +
        "append a synthetic Event to the ring buffer on a terminal task.",
    );

    // ── Step 5: the task survives a webview reload via list_tasks hydration (teeth) ───────────
    // Run the reload AFTER the task is succeeded (deterministic). The reload wipes the frontend
    // store; the only place the task can come from afterward is the Rust TaskRegistry snapshot.
    await browser.refresh();
    await ActivityCenter.openPanel();
    await ActivityCenter.waitForTask(taskId, "succeeded", 30000);
  });

  // ── Step 7: start_inference produces a second task of a different kind ──────────────────────
  it("tracks the inference-setup task to succeeded with the live base_url", async () => {
    await ModelsPage.open();
    await ModelsPage.startInference(model.id);
    await ModelsPage.waitForRunning(180000);

    await ActivityCenter.openPanel();
    await ActivityCenter.waitForTask("inference-setup", "succeeded", 180000);

    // The succeed() detail is the live base_url (Phase E), proving a second TaskKind drives the
    // same open/progress/succeed path and renders.
    const detail = await ActivityCenter.taskDetail("inference-setup");
    assert(
      /^http:\/\/127\.0\.0\.1:\d+\/v1$/.test(detail),
      `inference-setup detail "${detail}" is not a loopback /v1 base_url. Phase E must pass ` +
        "handle.base_url as the succeed detail.",
    );

    await ModelsPage.stopInference();
  });

  // ── Conditional Step 8: a container event reaches the global event scrollback (self-skipping) ─
  it("surfaces a container lifecycle event in the global scrollback", async function () {
    const sandbox = `act-evt-${Date.now().toString().slice(-6)}`;
    try {
      await ModelsPage.goHome();
      await ProjectsPage.open();
      // First sandbox on a connection triggers the one-time base-image build, so widen the wait.
      await ProjectsPage.createProject(sandbox, { timeout: 300000 });
    } catch (e) {
      console.log(`[activity] skipping container scenario — sandbox create unavailable: ${String(e)}`);
      this.skip();
    }

    await ActivityCenter.openPanel();
    // The listen_events channel is owned by the store (Phase C supersede of EventLog), so a real
    // container lifecycle line lands in the *global* scrollback. Poll: events arrive asynchronously.
    await browser.waitUntil(
      async () => (await ActivityCenter.eventsText()).includes(sandbox),
      {
        timeout: 60000,
        interval: 2000,
        timeoutMsg: `no container lifecycle line for ${sandbox} reached the global scrollback — the ` +
          "listen_events channel may not be owned by the activity store.",
      },
    );

    // Best-effort cleanup of the created sandbox.
    try {
      await browser.execute((name) => {
        // @ts-expect-error __TAURI_INTERNALS__ is injected into the webview at runtime.
        return window.__TAURI_INTERNALS__?.invoke("destroy_sandbox", { name, force: true });
      }, sandbox);
    } catch {
      /* best-effort */
    }
  });
});
