import assert from "node:assert";

import ModelsPage from "../../pageobjects/models.page.js";

// LIGHT spec for the models-page-caching feature (docs/features/models-page-caching/03_e2e.md).
// Exercises the real detect_machine_specs / redetect_machine_specs Tauri commands (Phases A–D)
// and the /models spinner + manual re-detect button (Phase E) against the mock backend —
// MIO_MOCK_BACKEND=1 swaps SandboxBackend/RuntimeManager/SyncEngine, none of which
// detect_machine_specs touches, so the PowerShell/winget probe underneath it runs for real. No
// podman machine, no MIO_TEST_MODEL, matching local-llm-ui.spec.ts's topology.

type MachineSpecs = {
  cpuCores: number;
  totalRamBytes: number;
  freeRamBytes: number;
  gpuNames: string[];
  hasNpu: boolean;
  arch: string;
  foundryInstalled: boolean;
  foundryInstallable: boolean;
};

async function invokeTimed(command: string): Promise<{ specs: MachineSpecs; elapsedMs: number }> {
  const start = Date.now();
  const specs = await browser.execute((cmd) => {
    // @ts-expect-error __TAURI_INTERNALS__ is injected into the webview at runtime.
    return window.__TAURI_INTERNALS__.invoke(cmd);
  }, command);
  return { specs: specs as MachineSpecs, elapsedMs: Date.now() - start };
}

describe("Models page caching — MachineSpecsCache (no download)", function () {
  this.timeout(60000);

  it("redetect_machine_specs forces a full re-probe with valid data", async () => {
    await ModelsPage.goHome();
    await ModelsPage.open();

    const { specs } = await invokeTimed("redetect_machine_specs");
    assert(specs.cpuCores > 0, `cpuCores must be > 0, got ${specs.cpuCores}`);
    assert(specs.totalRamBytes > 0, `totalRamBytes must be > 0, got ${specs.totalRamBytes}`);
  });

  it("a cache hit is dramatically faster than the forced re-probe, and serves identical static fields", async () => {
    await ModelsPage.goHome();
    await ModelsPage.open();

    // Force-warm a known-cold cache, then immediately read it back.
    const baseline = await invokeTimed("redetect_machine_specs");
    const cached = await invokeTimed("detect_machine_specs");

    // Relative bound, not a fixed millisecond threshold, so this is robust to slower CI hardware
    // (see docs/features/models-page-caching/03_e2e.md — De-risking Spike Results). Only apply the
    // relative check once the baseline itself shows a meaningful gap to measure.
    if (baseline.elapsedMs > 200) {
      assert(
        cached.elapsedMs < baseline.elapsedMs / 3,
        `cache hit (${cached.elapsedMs}ms) was not dramatically faster than the forced re-probe ` +
          `(${baseline.elapsedMs}ms) — the static-field cache may not be wired up.`,
      );
    }

    // The assertion with teeth: the cache must have served *stored* values, not coincidentally
    // re-probed the same numbers. Compare gpuNames by set equality — device enumeration order is
    // not guaranteed stable across two probes.
    assert.strictEqual(cached.specs.cpuCores, baseline.specs.cpuCores, "cpuCores changed between calls");
    assert.deepStrictEqual(
      [...cached.specs.gpuNames].sort(),
      [...baseline.specs.gpuNames].sort(),
      "gpuNames changed between calls",
    );
    assert.strictEqual(cached.specs.hasNpu, baseline.specs.hasNpu, "hasNpu changed between calls");
    assert.strictEqual(cached.specs.arch, baseline.specs.arch, "arch changed between calls");
    assert.strictEqual(
      cached.specs.foundryInstalled,
      baseline.specs.foundryInstalled,
      "foundryInstalled changed between calls",
    );
    assert.strictEqual(
      cached.specs.foundryInstallable,
      baseline.specs.foundryInstallable,
      "foundryInstallable changed between calls",
    );

    // Volatile fields are still read live on every call (never folded into the cached struct) —
    // asserted present, not asserted equal: free RAM may or may not shift between two calls.
    assert(cached.specs.totalRamBytes > 0, "totalRamBytes must be > 0 on a cache hit too");
    assert(cached.specs.freeRamBytes > 0, "freeRamBytes must be > 0 on a cache hit too");
  });

  it("a second /models visit resolves fast once the cache is warm", async () => {
    await ModelsPage.goHome();
    await ModelsPage.open();
    // Force-warm the cache explicitly so this assertion never depends on suite/spec ordering.
    await invokeTimed("redetect_machine_specs");

    await ModelsPage.goHome();
    const start = Date.now();
    await ModelsPage.open();
    const elapsedMs = Date.now() - start;
    assert(
      elapsedMs < 1000,
      `second /models visit took ${elapsedMs}ms to reach [data-specs-ready] — expected a warm-cache ` +
        "visit to resolve in under 1s.",
    );
  });

  it("the manual re-detect button drives a real re-probe from the UI", async () => {
    await ModelsPage.goHome();
    await ModelsPage.open();

    await ModelsPage.redetect();
    const btn = await ModelsPage.redetectButton;
    assert.strictEqual(
      await btn.getAttribute("aria-busy"),
      "true",
      "redetect button did not enter a busy state while the forced re-probe is in flight",
    );

    await browser.waitUntil(async () => (await btn.getAttribute("aria-busy")) !== "true", {
      timeout: 15000,
      interval: 500,
      timeoutMsg: "redetect button stayed busy 15s after clicking — redetect_machine_specs did not resolve",
    });
    await (await ModelsPage.specsReady).waitForDisplayed({ timeout: 5000 });
  });
});
