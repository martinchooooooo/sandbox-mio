import assert from "node:assert";
import path from "node:path";

import { captureDiagOnFailure } from "../../../lib/diag.js";
import { invokeCmd, useMockControl } from "../../../lib/mock.js";
import { WORKSPACE_ROOT } from "../../../lib/run-identity.js";
import { COMMAND, PANEL_RENDER } from "../../../lib/timeouts.js";

// LIGHT spec for native-terminal (docs/features/native-terminal/02_dev.md, Phase D). Under
// MIO_MOCK_BACKEND=1, default_terminal_host() builds MockTerminalHost (src-tauri/src/terminal/mock.rs),
// which prints a banner, echoes input and prints resizes; the real terminal_* commands, the
// terminal layer and the wterm renderer run unmodified.

describe("Native terminal — echo mock (mock backend)", function () {
  this.timeout(120000);

  useMockControl();
  captureDiagOnFailure();

  const name = `e2e-term-${Date.now()}`;
  const view = () => $(`[data-terminal='${name}']`);

  async function screenText(): Promise<string> {
    return (await (await view()).getText()) ?? "";
  }

  after(async () => {
    await invokeCmd("close_pane", { name, kind: "terminal" }).catch(() => undefined);
  });

  it("opens as a native pane and attaches with the renderer's measured size", async () => {
    await invokeCmd("open_terminal_pane", { name, workspacePath: path.join(WORKSPACE_ROOT, name) });
    const state = await invokeCmd<{ apps: { sandbox: string; kind: string; url: string | null; forward_name: string | null }[] }>(
      "workspace_state",
    );
    const pane = state.apps.find((p) => p.sandbox === name && p.kind === "terminal");
    assert.ok(pane, "no terminal pane registered");
    assert.strictEqual(pane.url, null);
    assert.strictEqual(pane.forward_name, null);

    await (await view()).waitForExist({ timeout: PANEL_RENDER });
    await browser.waitUntil(async () => (await (await view()).getAttribute("data-status")) === "attached", {
      timeout: PANEL_RENDER,
      timeoutMsg: "the terminal never attached",
    });
    await browser.waitUntil(async () => (await screenText()).includes(`mio mock terminal ${name}`), {
      timeout: COMMAND,
      timeoutMsg: "the mock banner never rendered",
    });
  });

  it("sends typed input and renders the echo", async () => {
    await browser.keys("echo hi");
    await browser.waitUntil(async () => (await screenText()).includes("echo hi"), {
      timeout: COMMAND,
      timeoutMsg: "typed input was not echoed",
    });
  });

  it("detaches when the pane closes", async () => {
    await invokeCmd("close_pane", { name, kind: "terminal" });
    await browser.waitUntil(async () => !(await (await view()).isExisting()), {
      timeout: COMMAND,
      timeoutMsg: "the terminal view stayed mounted after its pane closed",
    });
    await assert.rejects(invokeCmd("terminal_input", { name, data: "x" }));
  });
});
