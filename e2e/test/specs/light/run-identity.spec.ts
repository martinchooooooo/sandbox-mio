import assert from "node:assert";
import fs from "fs";
import os from "os";
import path from "path";

import { invokeCmd } from "../../../lib/mock.js";
import { SUBPROCESS } from "../../../lib/timeouts.js";
import {
  RUN_SLUG,
  WORKTREE_ROOT,
  APP_DATA_ROOT,
  WORKSPACE_ROOT,
  PORT_WINDOW_BASE,
  PORT_WINDOW_SIZE,
  deriveSlug,
} from "../../../lib/run-identity.js";

// Proves the app run alongside this spec actually honours the isolation
// e2e/wdio.conf.ts's launcher set up — not that e2e/lib/run-identity.ts merely computes sensible
// values (recomputing a path from the module that produced it and comparing the two would prove
// nothing; docs/features/parallel-worktree-e2e/03_e2e.md). No useMockControl() — this spec installs
// no overrides — and no captureDiagOnFailure(), following standard/base-image-recipe.spec.ts: every
// assertion here is a filesystem/environment check, not a DOM one.

// Watermark for the legacy-path check below: onPrepare relinks the debug exe on every run, so its
// mtime is the last moment before this run's app could have written anything. A wall-clock stamp
// taken here would be useless — the app starts in beforeSession, before this file is even loaded.
const RUN_STARTED_AT = fs.statSync(
  path.resolve(WORKTREE_ROOT, "src-tauri", "target", "debug", `sandbox-mio${process.platform === "win32" ? ".exe" : ""}`),
).mtimeMs;

describe("Run identity — per-worktree isolation", () => {
  it("derives a deterministic, path-derived, discriminating slug", () => {
    const again = deriveSlug(WORKTREE_ROOT);
    assert.strictEqual(again, RUN_SLUG, "deriveSlug(WORKTREE_ROOT) must be stable across calls.");

    const synthetic = deriveSlug(path.join(WORKTREE_ROOT, "..", "a-different-worktree"));
    assert.notStrictEqual(
      synthetic,
      RUN_SLUG,
      "Two distinct worktree roots produced the same slug — onPrepare's cleanup would then be able " +
        "to delete a sibling worktree's directories.",
    );

    assert(RUN_SLUG.length > 0, "RUN_SLUG must be non-empty.");
    assert(
      /^[A-Za-z0-9._-]+$/.test(RUN_SLUG),
      `RUN_SLUG "${RUN_SLUG}" contains characters illegal in a Windows path segment or filename.`,
    );
  });

  it("keys the app-data and workspace roots to the slug, and keeps them distinct", () => {
    const settingsDir = path.join(APP_DATA_ROOT, "settings");
    const credentialsDir = path.join(APP_DATA_ROOT, "credentials");
    const roots = [APP_DATA_ROOT, WORKSPACE_ROOT, settingsDir, credentialsDir];

    for (const root of roots) {
      assert(
        root.includes(RUN_SLUG),
        `${root} does not contain RUN_SLUG (${RUN_SLUG}) — a future edit may have re-rooted it at a fixed path.`,
      );
    }
    assert.strictEqual(
      new Set(roots).size,
      roots.length,
      `Expected APP_DATA_ROOT, WORKSPACE_ROOT, settings, and credentials to be pairwise distinct; got ${JSON.stringify(roots)}.`,
    );
  });

  it("never wrote to the pre-feature fixed app-data path", () => {
    const legacyAppData = path.join(os.homedir(), ".sandbox-mio-e2e-appdata");
    if (!fs.existsSync(legacyAppData)) return;
    // Non-modification rather than absence: every machine that ran the suite before this feature
    // still has the directory left over, and asserting it is gone would fail for a reason unrelated
    // to the code under test. What matters is that *this* run did not write there.
    assert(
      fs.statSync(legacyAppData).mtimeMs < RUN_STARTED_AT,
      `${legacyAppData} was modified during this run — the app (or the launcher) fell through to a ` +
        "fixed pre-feature path instead of the slug-keyed MIO_APP_DATA_DIR the launcher set.",
    );
  });

  it("persists settings under the slug-keyed app-data root", async () => {
    const settingsPath = path.join(APP_DATA_ROOT, "settings", "settings.json");
    const before = await invokeCmd<{ llamaCtxSize?: number | null }>("get_settings");
    // A value local-llm-ui.spec.ts's inferenceEngine toggle never touches, so the two specs cannot
    // stomp each other's assertion within the app-data root they share for this run.
    const probe = ((before.llamaCtxSize ?? 0) === 20481) ? 20482 : 20481;

    await invokeCmd("set_settings", { settings: { ...before, llamaCtxSize: probe } });
    try {
      assert(
        fs.existsSync(settingsPath),
        `${settingsPath} was not created by set_settings — MIO_SETTINGS_DIR is not reaching settings::save.`,
      );
      const onDisk = JSON.parse(fs.readFileSync(settingsPath, "utf8"));
      assert.strictEqual(
        onDisk.llamaCtxSize,
        probe,
        `settings.json under APP_DATA_ROOT shows llamaCtxSize=${onDisk.llamaCtxSize}, expected ${probe}.`,
      );
    } finally {
      // Restore, so a later spec in this run sees the settings it expects.
      await invokeCmd("set_settings", { settings: before });
    }
  });

  it("writes mio.log under the slug-keyed app-data root", async () => {
    const logsDir = path.join(APP_DATA_ROOT, "logs");
    await browser.waitUntil(
      () => {
        if (!fs.existsSync(logsDir)) return false;
        // tracing_appender::rolling::daily suffixes the base name with the date
        // (mio.log.<YYYY-MM-DD>), so match on a prefix rather than the exact name.
        const entry = fs.readdirSync(logsDir).find((f) => f.startsWith("mio.log"));
        if (!entry) return false;
        return fs.statSync(path.join(logsDir, entry)).size > 0;
      },
      {
        timeout: SUBPROCESS,
        timeoutMsg: `No non-empty mio.log.* appeared under ${logsDir} in time. This is the only ` +
          "automated proof of the MIO_APP_DATA_DIR override branch in init_logging() — settings and " +
          "credentials are covered by their own higher-precedence variables and would pass whether or " +
          "not this branch works.",
      },
    );
  });

  it("uses the launcher-allocated port pair for this session", () => {
    const envPort = Number(process.env.MIO_DRIVER_PORT);
    assert(
      Number.isFinite(envPort) && envPort > 0,
      `process.env.MIO_DRIVER_PORT is "${process.env.MIO_DRIVER_PORT}" in this worker — the port ` +
        "allocated by onPrepare was not inherited.",
    );
    assert.strictEqual(
      browser.options.port,
      envPort,
      `The running session is on port ${browser.options.port}, but MIO_DRIVER_PORT is ${envPort} — ` +
        "the port the launcher logged is not the port actually in force (a module-scope re-probe " +
        "would produce exactly this divergence).",
    );
    assert(
      envPort >= PORT_WINDOW_BASE && envPort < PORT_WINDOW_BASE + PORT_WINDOW_SIZE,
      `Port ${envPort} falls outside this worktree's own window ` +
        `[${PORT_WINDOW_BASE}, ${PORT_WINDOW_BASE + PORT_WINDOW_SIZE}).`,
    );
  });

  it("passes the slug-keyed workspace root to the app", () => {
    assert.strictEqual(
      process.env.SANDBOX_WORKSPACE_ROOT,
      WORKSPACE_ROOT,
      `SANDBOX_WORKSPACE_ROOT in this worker's environment is "${process.env.SANDBOX_WORKSPACE_ROOT}", ` +
        `expected the slug-keyed ${WORKSPACE_ROOT}.`,
    );
  });
});
