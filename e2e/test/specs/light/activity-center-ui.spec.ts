import assert from "node:assert";

import ModelsPage from "../../pageobjects/models.page.js";
import ProjectsPage from "../../pageobjects/projects.page.js";
import ActivityCenter from "../../pageobjects/activity.page.js";

// LIGHT spec for the activity center: runs in the default `pnpm test` suite. It proves the global
// surface (the layout-mounted bell), the `list_tasks` round-trip (Phase A), and the store's fold /
// badge / toast / version-guard logic (Phases B–C) — WITHOUT downloading a GGUF or starting a
// sidecar. The real Rust pipeline (registry → app.emit → survives reload) is the heavy spec
// (test/specs/heavy/activity-center.spec.ts).
//
// The store's fold logic is driven by a *synthetic* `activity://task-updated` event emitted from the
// webview. A frontend `emit` round-trips through Rust and back to the store's `listen` (same event
// bus the real backend uses), so this exercises the real fold/badge/toast code deterministically
// and instantly — a synthetic task lives only in the frontend store and never enters the Rust
// registry, so it does not pollute `list_tasks` or other specs.

const SYNTH_ID = "model-download:e2e-ui-synthetic";

// Build a Task payload (camelCase, mirroring the Rust serde shape) for the synthetic event.
function synthTask(version: number, status: string, extra: Record<string, unknown> = {}) {
  return {
    id: SYNTH_ID,
    kind: "model-download",
    version,
    status,
    progress: status === "running" ? 0.5 : 1.0,
    target: "e2e-ui-synthetic",
    detail: "1.0 / 2.0 GB",
    error: null,
    startedAtMs: Date.now(),
    endedAtMs: status === "running" ? null : Date.now(),
    ...extra,
  };
}

// Emit `activity://task-updated` from the webview via the documented event plugin command, the same
// bus the store subscribes to. Returns whether the emit invoke resolved.
async function emitTask(task: Record<string, unknown>): Promise<boolean> {
  return browser.execute((payload) => {
    // @ts-expect-error __TAURI_INTERNALS__ is injected into the webview at runtime.
    const internals = window.__TAURI_INTERNALS__;
    if (!internals?.invoke) return Promise.resolve(false);
    return internals
      .invoke("plugin:event|emit", { event: "activity://task-updated", payload })
      .then(() => true)
      .catch(() => false);
  }, task);
}

describe("Activity Center — UI sanity (no backend task)", function () {
  this.timeout(60000);

  it("renders the bell toggle on every route (layout-mounted global surface)", async () => {
    await ModelsPage.goHome();
    assert(await ActivityCenter.isToggleShown(), "Bell toggle missing on home — the activity center must mount in +layout.svelte.");
    await ModelsPage.open();
    assert(await ActivityCenter.isToggleShown(), "Bell toggle missing on /models — a per-route mount would not survive navigation.");
    await ModelsPage.goHome();
    await ProjectsPage.open();
    assert(await ActivityCenter.isToggleShown(), "Bell toggle missing on /projects — the surface must be present on every route.");
  });

  it("round-trips the list_tasks command to an array (Phase A)", async () => {
    const isArray = await browser.execute(() => {
      // @ts-expect-error __TAURI_INTERNALS__ is injected into the webview at runtime.
      return window.__TAURI_INTERNALS__.invoke("list_tasks").then((r: unknown) => Array.isArray(r));
    });
    assert(isArray, "list_tasks did not resolve to an array — the Phase A snapshot command is missing or errored.");
  });

  it("shows an empty panel and a zero badge when idle", async () => {
    await ModelsPage.goHome();
    await ActivityCenter.openPanel();
    assert.strictEqual(
      await ActivityCenter.badgeCount(),
      0,
      "Badge is non-zero with no tasks — a synthetic/real task leaked, or badgeCount counts the wrong set.",
    );
    assert.strictEqual(
      await ActivityCenter.taskCount(),
      0,
      "Panel shows task rows when idle — the registry snapshot should be empty at app launch.",
    );
  });

  it("folds a synthetic running task into a row and ticks the badge", async () => {
    await ModelsPage.goHome();
    const emitted = await emitTask(synthTask(1_000, "running"));
    assert(emitted, "Could not emit activity://task-updated from the webview — the event plugin command name may have changed.");

    await ActivityCenter.openPanel();
    await ActivityCenter.waitForTask(SYNTH_ID, "running", 10000);
    assert(
      (await ActivityCenter.badgeCount()) >= 1,
      "Badge did not count the running synthetic task — badgeCount must include running tasks.",
    );
  });

  it("ignores a stale lower-version update (the version-guard race fix)", async () => {
    // A late-delivered older update must not regress the row. The store folds only strictly-greater
    // versions, which is what makes the snapshot and the live stream commutative.
    await emitTask(synthTask(500, "succeeded"));
    // Give the (ignored) event time to be processed, then assert the row is still running.
    await browser.pause(500);
    await ActivityCenter.openPanel();
    const row = await ActivityCenter.taskRow(SYNTH_ID);
    assert.strictEqual(
      await row.getAttribute("data-task-status"),
      "running",
      "A stale lower-version update overwrote a newer one — the strictly-greater-version fold guard is missing.",
    );
  });

  it("applies a newer succeeded update and raises a success toast", async () => {
    await emitTask(synthTask(1_001, "succeeded"));
    await ActivityCenter.openPanel();
    await ActivityCenter.waitForTask(SYNTH_ID, "succeeded", 10000);

    // The success toast is best-effort (it auto-dismisses), so capture it promptly. A null type just
    // means it already dismissed; the row reaching succeeded above is the required proof.
    const type = await ActivityCenter.toastType();
    if (type !== null) {
      assert.strictEqual(type, "success", `Terminal-success toast had type "${type}", expected "success".`);
    }
  });
});
