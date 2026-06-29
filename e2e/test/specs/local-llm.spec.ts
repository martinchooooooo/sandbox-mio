import assert from "node:assert";
import fs from "fs";
import path from "path";

import ModelsPage from "../pageobjects/models.page.js";

// Host-only spec (Steps 1–5 of docs/features/local-llm/03_e2e.md): no Podman. It drives the real
// detect → download → start → completion → stop path of the llama engine against a tiny test GGUF
// injected via MIO_TEST_MODEL, and asserts on the host filesystem and a real HTTP call to the
// inference endpoint rather than `podman exec`.

interface TestModel {
  id: string;
  label: string;
  repo: string;
  file: string;
  sha256: string;
  sizeBytes: number;
}

// The debug build writes models under app_data_dir() = %APPDATA%/<bundle identifier>/models.
// The identifier is com.tincho.sandbox-mio (src-tauri/tauri.conf.json).
const APP_DATA_MODELS = path.join(
  process.env.APPDATA ?? path.join(process.env.USERPROFILE ?? "", "AppData", "Roaming"),
  "com.tincho.sandbox-mio",
  "models",
);

describe("Local LLM — llama engine", function () {
  this.timeout(600000); // cold run downloads the test GGUF and loads it into the sidecar

  let model: TestModel;
  let modelPath: string;

  before(() => {
    const raw = process.env.MIO_TEST_MODEL;
    assert(
      raw,
      "MIO_TEST_MODEL is not set. The spec needs a tiny test GGUF descriptor " +
        "{id,label,repo,file,sha256,sizeBytes}; wdio.conf.ts must export it into the spec env.",
    );
    model = JSON.parse(raw) as TestModel;
    modelPath = path.join(APP_DATA_MODELS, model.id, model.file);
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

  // Capture webview state on any failure (identical to runtime-management.spec.ts).
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

  it("probes real host specs (not the 2 GiB container VM)", async () => {
    await ModelsPage.open();
    const specs = await ModelsPage.specsText();
    assert(
      /\d/.test(specs) && /RAM/i.test(specs),
      `Specs panel "${specs}" shows no RAM/CPU figures. detect_machine_specs must return host ` +
        "numbers read via sysinfo; an empty panel means the probe command never resolved.",
    );
    assert(
      /[1-9]\d*\s*CPU/i.test(specs),
      `Specs panel "${specs}" shows zero CPU cores. A zero count means the host probe failed.`,
    );
  });

  it("downloads the test GGUF to the app data dir", async () => {
    await ModelsPage.startDownload(model.id);
    await ModelsPage.waitForDownloaded(model.id, 480000);

    assert(
      fs.existsSync(modelPath),
      `Test GGUF ${modelPath} missing after download. This can happen when download_model failed ` +
        "the sha256 verify and deleted the .part file, or wrote to a different app-data path.",
    );
    const size = fs.statSync(modelPath).size;
    assert(
      size === model.sizeBytes,
      `Downloaded GGUF is ${size} bytes, expected ${model.sizeBytes}. A size mismatch means a ` +
        "truncated download passed the presence check; download_model must verify sha256 before rename.",
    );
  });

  it("starts the bundled sidecar, ticks the setup checklist, and serves a completion", async () => {
    await ModelsPage.startInference(model.id);
    await ModelsPage.waitForRunning(180000);

    // 4z. The runtime-setup checklist rendered with non-empty, resolved titles.
    const titles = await ModelsPage.stepTitles();
    assert(
      titles.length > 0 && titles.every((t) => t.length > 0),
      `Runtime-setup checklist rendered ${titles.length} rows; titles=${JSON.stringify(titles)}. ` +
        "A blank row means a step id was added without its Paraglide runtime_step_<id>_title key.",
    );

    // 4a. base_url is a loopback /v1 endpoint.
    const baseUrl = await ModelsPage.baseUrl();
    assert(
      /^http:\/\/127\.0\.0\.1:\d+\/v1$/.test(baseUrl),
      `base_url "${baseUrl}" is not a loopback /v1 endpoint. start_inference must report the live ` +
        "sidecar port.",
    );

    // 4b. A real chat completion returns non-empty content — the assertion with teeth.
    const res = await fetch(`${baseUrl}/chat/completions`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        model: model.id,
        messages: [{ role: "user", content: "Say hello in one word." }],
        max_tokens: 16,
      }),
    });
    assert(
      res.ok,
      `chat/completions returned HTTP ${res.status}. The bundled llama-server sidecar did not ` +
        "serve the request — model load may have failed.",
    );
    const body = (await res.json()) as { choices?: { message?: { content?: string } }[] };
    const content = body?.choices?.[0]?.message?.content ?? "";
    assert(
      content.trim().length > 0,
      "chat/completions returned empty content. The sidecar accepted the connection but produced " +
        "no tokens — the GGUF may be corrupt or architecture-mismatched with the bundled llama-server.",
    );
  });

  it("stops inference and the endpoint stops answering", async () => {
    const baseUrl = await ModelsPage.baseUrl();
    await ModelsPage.stopInference();

    let reachable = true;
    try {
      await fetch(`${baseUrl}/models`, { signal: AbortSignal.timeout(2000) });
    } catch {
      reachable = false;
    }
    assert(
      !reachable,
      `Endpoint ${baseUrl} still answered after stop_inference. The sidecar was not terminated — ` +
        "it would orphan and hold RAM after the app exits.",
    );
  });
});
