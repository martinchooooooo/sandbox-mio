import assert from "node:assert";
import path from "node:path";

import { captureDiagOnFailure } from "../../../lib/diag.js";
import { configureMock, invokeCmd, mockCalls, useMockControl } from "../../../lib/mock.js";
import { WORKSPACE_ROOT } from "../../../lib/run-identity.js";
import { COMMAND, PANEL_RENDER } from "../../../lib/timeouts.js";

// LIGHT spec for the native terminal's attach lifecycle (docs/features/native-terminal/02_dev.md).
// Under MIO_MOCK_BACKEND=1 the mock transport (src-tauri/src/terminal/mock.rs) runs inside the same
// LifecycleHost as the SSH transport, so scripting `terminal.open` drives the real attach race:
// pending, promote, cancel, supersede and abandon.

/** How long a scripted open is held; long enough to act while an attach is pending. */
const HOLD_MS = 4_000;
/** Slack after a held open's deadline, for the open to finish and its result to be handled. */
const SETTLE_MS = 1_500;

describe("Native terminal — attach lifecycle (mock backend)", function () {
  this.timeout(120000);

  useMockControl();
  captureDiagOnFailure();

  let name = "";
  let seq = 0;
  const view = () => $(`[data-terminal='${name}']`);

  async function status(): Promise<string | undefined> {
    const el = await view();
    return (await el.isExisting()) ? ((await el.getAttribute("data-status")) ?? undefined) : undefined;
  }

  async function screenText(): Promise<string> {
    return (await (await view()).getText()) ?? "";
  }

  async function openPane(): Promise<void> {
    await invokeCmd("open_terminal_pane", { name, workspacePath: path.join(WORKSPACE_ROOT, name) });
  }

  async function closePane(): Promise<void> {
    await invokeCmd("close_pane", { name, kind: "terminal" });
    await browser.waitUntil(async () => !(await (await view()).isExisting()), {
      timeout: COMMAND,
      timeoutMsg: "the terminal view stayed mounted after its pane closed",
    });
  }

  async function waitForStatus(expected: string): Promise<void> {
    await browser.waitUntil(async () => (await status()) === expected, {
      timeout: PANEL_RENDER,
      timeoutMsg: `the terminal never reached data-status=${expected}`,
    });
  }

  /** Resolves with the time at which the nth `terminal.open` was journalled, which is when it began. */
  async function waitForOpen(n: number): Promise<number> {
    await browser.waitUntil(async () => (await mockCalls("terminal.open")).length >= n, {
      timeout: PANEL_RENDER,
      timeoutMsg: `terminal.open was not called ${n} time(s)`,
    });
    return Date.now();
  }

  /** Waits until a held open that began at `startedAt` has finished and been handled. */
  async function waitForHeldOpen(startedAt: number): Promise<void> {
    await browser.waitUntil(async () => Date.now() - startedAt >= HOLD_MS + SETTLE_MS, {
      timeout: HOLD_MS + SETTLE_MS + COMMAND,
      interval: 250,
      timeoutMsg: "the held open's deadline never passed",
    });
  }

  async function inputRejects(): Promise<boolean> {
    return browser.execute(async (n) => {
      try {
        // @ts-expect-error __TAURI_INTERNALS__ is injected into the webview at runtime.
        await window.__TAURI_INTERNALS__.invoke("terminal_input", { name: n, data: "x" });
        return false;
      } catch {
        return true;
      }
    }, name);
  }

  beforeEach(() => {
    name = `e2e-term-life-${Date.now()}-${++seq}`;
  });

  afterEach(async () => {
    await invokeCmd("close_pane", { name, kind: "terminal" }).catch(() => undefined);
  });

  it("closing the pane while it connects cancels the attach and leaks no session", async () => {
    await configureMock("terminal.open", { kind: "delay", ms: HOLD_MS });
    await openPane();
    await waitForStatus("connecting");
    const startedAt = await waitForOpen(1);
    assert.ok(await inputRejects(), "input must be rejected while the attach is pending");

    await closePane();
    await waitForHeldOpen(startedAt);

    assert.ok(await inputRejects(), "a cancelled attach left a live session behind");
    assert.strictEqual((await mockCalls("terminal.open")).length, 1, "the cancelled attach was retried");
  });

  it("reopening the pane while the first attach is held lets the newest attach win", async () => {
    // Only the first open is held, so the second finishes first and the first finishes stale.
    await configureMock("terminal.open", { kind: "delay", ms: HOLD_MS, calls: 1 });
    await browser.execute((n) => {
      const seen: string[] = [];
      // @ts-expect-error test-only probe read back below.
      window.__termStatuses = seen;
      const record = () => {
        const el = document.querySelector(`[data-terminal='${n}']`);
        const current = el?.getAttribute("data-status");
        if (current && seen[seen.length - 1] !== current) seen.push(current);
      };
      new MutationObserver(record).observe(document.body, {
        subtree: true,
        childList: true,
        attributes: true,
        attributeFilter: ["data-status"],
      });
      record();
    }, name);

    await openPane();
    await waitForStatus("connecting");
    const startedAt = await waitForOpen(1);
    await closePane();
    await openPane();
    await waitForStatus("attached");
    await waitForHeldOpen(startedAt);

    assert.strictEqual(await status(), "attached", "the stale attach replaced or ended the newer session");
    await browser.keys("echo hi");
    await browser.waitUntil(async () => (await screenText()).includes("echo hi"), {
      timeout: COMMAND,
      timeoutMsg: "typed input was not echoed after the stale attach finished",
    });
    const seen = await browser.execute(() => {
      // @ts-expect-error test-only probe set above.
      return window.__termStatuses as string[];
    });
    assert.ok(seen.includes("attached"), `statuses seen: ${seen.join(",")}`);
    assert.ok(!seen.includes("ended"), `the view showed the ended state: ${seen.join(",")}`);
    assert.strictEqual((await mockCalls("terminal.open")).length, 2);
  });

  it("a failed open shows its error, leaves no session, and Retry attaches", async () => {
    const stderr = "scripted: the sandbox refused the terminal command";
    await configureMock("terminal.open", { kind: "fail-once", exit_code: 1, stderr });
    await openPane();
    await waitForStatus("failed");
    const error = await $(`[data-terminal='${name}'] [data-terminal-error]`);
    await error.waitForExist({ timeout: COMMAND });
    assert.ok((await error.getText()).includes(stderr), "the scripted message was not shown");
    assert.ok(await inputRejects(), "a failed open left a session behind");

    await (await $(`[data-terminal='${name}'] [data-terminal-retry]`)).click();
    await waitForStatus("attached");
    await browser.keys("echo ok");
    await browser.waitUntil(async () => (await screenText()).includes("echo ok"), {
      timeout: COMMAND,
      timeoutMsg: "typed input was not echoed after the retry",
    });
    assert.strictEqual((await mockCalls("terminal.open")).length, 2);
  });

  it("delivers a long burst of typed input exactly and in order", async () => {
    await openPane();
    await waitForStatus("attached");
    // Distinct three-digit hex groups, so any reordering or loss changes the sequence.
    const burst = Array.from({ length: 80 }, (_, i) => i.toString(16).padStart(3, "0")).join("");
    await browser.keys(burst);
    // The renderer wraps long lines onto several rows, so compare with whitespace removed.
    await browser.waitUntil(async () => (await screenText()).replace(/\s+/g, "").includes(burst), {
      timeout: COMMAND,
      timeoutMsg: "the burst was not echoed exactly and in order",
    });
  });

  it("rejects an invalid sandbox name at the boundary without opening a session", async () => {
    const error = await browser.execute(async (n) => {
      // @ts-expect-error __TAURI_INTERNALS__ is injected into the webview at runtime.
      const internals = window.__TAURI_INTERNALS__;
      const channel = () => `__CHANNEL__:${internals.transformCallback(() => undefined)}`;
      try {
        await internals.invoke("terminal_attach", { name: n, cols: 80, rows: 24, onOutput: channel(), onEvent: channel() });
        return null;
      } catch (e) {
        return e as { kind: string; message: string };
      }
    }, "x; rm -rf /");
    assert.ok(error, "terminal_attach accepted an invalid name");
    assert.strictEqual(error.kind, "Backend");
    assert.match(error.message, /invalid sandbox name/);
    assert.strictEqual((await mockCalls("terminal.open")).length, 0, "an invalid name reached the transport");
  });
});
