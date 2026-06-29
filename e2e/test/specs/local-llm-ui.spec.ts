import assert from "node:assert";

import ModelsPage from "../pageobjects/models.page.js";
import SettingsPage from "../pageobjects/settings.page.js";

// LIGHT spec for the local-llm feature: runs in the default `pnpm test` suite. It proves the
// routes render and the cheap Tauri commands round-trip (detect_machine_specs, downloaded_models,
// inference_status, runtime_setup_steps, get/set_settings, test_model) plus the catalogue/recommend
// and UI-gating logic — WITHOUT ever downloading a GGUF or starting a sidecar. The real
// download → load → completion path is the heavy spec (test/specs/heavy/local-llm.spec.ts).

// The test-only model descriptor injected by wdio.conf.ts (MIO_TEST_MODEL). Its row is prepended to
// the catalogue when set; the light spec only inspects its row, never downloads it.
const TEST_MODEL_ID = (() => {
  const raw = process.env.MIO_TEST_MODEL;
  return raw ? (JSON.parse(raw) as { id: string }).id : null;
})();

describe("Local LLM — UI sanity (no download)", function () {
  this.timeout(60000);

  it("navigates home → /models and renders the host-specs panel", async () => {
    await ModelsPage.goHome();
    await ModelsPage.open();
    const specs = await ModelsPage.specsText();
    assert(
      /\d/.test(specs) && /RAM/i.test(specs),
      `Specs panel "${specs}" shows no RAM figures. detect_machine_specs must return host numbers; ` +
        "an empty panel means the probe command never resolved or the route failed to mount.",
    );
  });

  it("renders the ranked catalogue with a rating per row, including the test model", async () => {
    await ModelsPage.goHome();
    await ModelsPage.open();
    const ids = await ModelsPage.modelIds();
    assert(ids.length > 0, "Catalogue rendered no model rows — recommendModels(specs) returned empty.");
    if (TEST_MODEL_ID) {
      assert(
        ids.includes(TEST_MODEL_ID),
        `Test model row "${TEST_MODEL_ID}" missing. The /models onMount prepends it when ` +
          "test_model (MIO_TEST_MODEL) is set; absence means that command or the env plumbing broke.",
      );
      const rating = await ModelsPage.rating(TEST_MODEL_ID);
      assert(rating.trim().length > 0, "Test model row shows no rating badge — recommend.ts produced no rating.");
    }
  });

  it("gates Start on a non-downloaded llama model", async () => {
    if (!TEST_MODEL_ID) return; // nothing injected to inspect
    await ModelsPage.goHome();
    await ModelsPage.open();
    const row = await ModelsPage.row(TEST_MODEL_ID);
    const downloaded = await (await row.$("[data-downloaded]")).isExisting();
    const startEnabled = await ModelsPage.startEnabled(TEST_MODEL_ID);
    // Gate logic (+page.svelte): a llama model that isn't downloaded must not be startable, and a
    // downloaded one must be. Either branch validates the disabled binding.
    assert(
      downloaded ? startEnabled : !startEnabled,
      `Start gate wrong: downloaded=${downloaded} but startEnabled=${startEnabled}. A non-downloaded ` +
        "llama model must keep Start disabled (the !isDownloaded guard).",
    );
  });

  it("shows no running/chat UI when nothing is running", async () => {
    await ModelsPage.goHome();
    await ModelsPage.open();
    assert(
      !(await ModelsPage.isRunningShown()),
      "A [data-running] panel exists with no server started — inference_status reported running:true " +
        "for an idle app, or stale state leaked from a prior run.",
    );
    assert(
      !(await ModelsPage.isChatShown()),
      "The [data-chat] test surface is shown while idle — it must only render when status.running.",
    );
  });

  it("renders the settings runtime checklist and persists the engine choice", async () => {
    await SettingsPage.goHome();
    await SettingsPage.open();

    // The static explainer checklist must render non-empty titles — the same i18n-key teeth as the
    // heavy spec, but instant: a blank row means a step id has no Paraglide runtime_step_<id>_title.
    const titles = await SettingsPage.stepTitles();
    assert(
      titles.length > 0 && titles.every((t) => t.length > 0),
      `Runtime checklist rendered ${titles.length} rows; titles=${JSON.stringify(titles)}. A blank ` +
        "row means a step id was added without its Paraglide runtime_step_<id>_title key.",
    );

    // Round-trip set_settings → get_settings: pick the engine the app is NOT currently on, save,
    // reload the route, and assert the choice persisted.
    const before = await SettingsPage.activeEngine();
    const next = before === "llama" ? "foundry" : "llama";
    await SettingsPage.selectEngine(next as "llama" | "foundry");
    await SettingsPage.save();

    await SettingsPage.goHome();
    await SettingsPage.open();
    const after = await SettingsPage.activeEngine();
    assert(
      after === next,
      `Engine did not persist: saved "${next}" but reload shows "${after}". set_settings/get_settings ` +
        "must round-trip the inferenceEngine field through the on-disk settings file.",
    );

    // Restore the original engine so the suite leaves settings as it found them.
    if (after !== before) {
      await SettingsPage.selectEngine(before as "llama" | "foundry");
      await SettingsPage.save();
    }
  });
});
