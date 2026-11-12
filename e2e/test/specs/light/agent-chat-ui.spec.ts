import assert from "node:assert";
import fs from "node:fs";
import path from "node:path";

import chat from "../../pageobjects/agent-chat.page.js";
import { captureDiagOnFailure } from "../../../lib/diag.js";
import { configureMock, invokeCmd, useMockControl } from "../../../lib/mock.js";
import { WORKSPACE_ROOT } from "../../../lib/run-identity.js";
import { COMMAND, PANEL_RENDER } from "../../../lib/timeouts.js";

// LIGHT spec for native-agent-ui (docs/features/native-agent-ui/03_e2e.md, spec L1). Under
// MIO_MOCK_BACKEND=1, default_agent_host() builds MockAgentHost (src-tauri/src/agent_host/mock.rs),
// which replays the spike's recorded pi traces; the real agent_chat_* commands, the chat store and
// the pi-events adapter run unmodified. The real supervisor path is the standard spec's job.

/** Mirrors CHAT_SESSION_DIR in src-tauri/src/agent_host/sessions.rs. */
const CHAT_SESSION_DIR = path.join(".mio", "pi-agent", "chat-sessions");

async function openChat(name: string): Promise<void> {
  await invokeCmd("open_agent_chat", { name, workspacePath: path.join(WORKSPACE_ROOT, name) });
  await (await chat.root(name)).waitForExist({ timeout: PANEL_RENDER });
  await chat.waitForConnection(name, "attached");
}

describe("Native agent chat — replayed pi traces (mock backend)", function () {
  this.timeout(120000);

  useMockControl();
  captureDiagOnFailure();

  const name = `e2e-chat-${Date.now()}`;

  after(async () => {
    await invokeCmd("close_pane", { name, kind: "agentchat" }).catch(() => undefined);
  });

  it("attaches as the writer and shows the model from get_state", async () => {
    await openChat(name);
    assert.strictEqual(await chat.attribute(name, "data-writer"), "true");
    assert.strictEqual(await chat.attribute(name, "data-run"), "idle");
    await (await chat.emptyState).waitForDisplayed({ timeout: COMMAND });
    await browser.waitUntil(async () => (await (await chat.modelButton).getAttribute("data-chat-model")) === "qwen3.5-2b", {
      timeout: COMMAND,
      timeoutMsg: "the model selector never showed qwen3.5-2b from get_state",
    });
    // The mock's models are reasoning models, so the thinking control renders at pi's level.
    assert.strictEqual(await (await chat.thinkingButton).getAttribute("data-chat-thinking"), "off");
  });

  it("streams a prompt's thinking and answer, then settles with usage", async () => {
    await chat.send("Which transport does the chat use?");
    await (await $("[data-role='user']")).waitForExist({ timeout: COMMAND });
    await (await $("[data-role='assistant']")).waitForExist({ timeout: PANEL_RENDER });
    await chat.waitForIdle(name);

    assert.match(await chat.lastAssistantText(), /named pipe/);
    assert.ok(await (await $("[data-part='reasoning']")).isExisting(), "no reasoning part rendered");
    // usage.totalTokens 1599 of the 32,768 window rounds to 5 %.
    assert.strictEqual(await (await chat.contextMeter).getAttribute("data-chat-context"), "5");
  });

  it("raises a confirm dialog from an extension command and resolves it", async () => {
    await chat.send("/spikeconfirm");
    const dialog = await chat.dialog("confirm");
    await dialog.waitForDisplayed({ timeout: PANEL_RENDER });
    assert.ok(await dialog.getAttribute("data-dialog-id"), "the dialog carries no request id");

    const yes = await dialog.$("[data-dialog-yes]");
    await yes.waitForClickable({ timeout: COMMAND });
    await yes.click();

    await browser.waitUntil(async () => (await chat.dialogCount()) === 0, {
      timeout: COMMAND,
      timeoutMsg: "the answered dialog stayed on screen",
    });
    await (await chat.toast).waitForDisplayed({ timeout: PANEL_RENDER });
  });

  it("re-seeds a pending dialog from HostStatus after a webview reload", async () => {
    await chat.send("/spikeconfirm");
    await (await chat.dialog("confirm")).waitForDisplayed({ timeout: PANEL_RENDER });

    // A reload drops the module-scoped store, so the next attach is fresh and the dialog can come
    // only from ensure's pending dialogs (docs/features/native-agent-ui/02_dev.md, Resolved Decision 3).
    await browser.refresh();
    await (await chat.root(name)).waitForExist({ timeout: PANEL_RENDER });
    await chat.waitForConnection(name, "attached");
    const dialog = await chat.dialog("confirm");
    await dialog.waitForDisplayed({ timeout: PANEL_RENDER });
    // History came back from get_messages, not from a journal replay.
    assert.ok((await chat.messageCount("assistant")) >= 1, "history was not rebuilt after the reload");

    await (await dialog.$("[data-dialog-yes]")).click();
    await browser.waitUntil(async () => (await chat.dialogCount()) === 0, {
      timeout: COMMAND,
      timeoutMsg: "the re-seeded dialog stayed on screen after answering",
    });
  });

  it("switches the model through the selector", async () => {
    await chat.pick(chat.modelButton, chat.modelOption("qwen3.5-4b"));
    await browser.waitUntil(async () => (await (await chat.modelButton).getAttribute("data-chat-model")) === "qwen3.5-4b", {
      timeout: COMMAND,
      timeoutMsg: "the model selector did not switch to qwen3.5-4b",
    });
  });

  it("lists a session file from the workspace and starts a new session", async () => {
    const id = `e2e-session-${Date.now()}`;
    const dir = path.join(WORKSPACE_ROOT, name, CHAT_SESSION_DIR, "--root-work--");
    fs.mkdirSync(dir, { recursive: true });
    const lines = [
      { type: "session", version: 3, id, cwd: "/root/work" },
      { type: "message", message: { role: "user", content: [{ type: "text", text: "seeded question" }] } },
      { type: "message", message: { role: "assistant", content: [{ type: "text", text: "seeded answer" }] } },
    ];
    fs.writeFileSync(path.join(dir, `${id}.jsonl`), lines.map((l) => JSON.stringify(l)).join("\n") + "\n");

    await (await chat.sessionsButton).click();
    await (await chat.sessionItem(id)).waitForDisplayed({ timeout: COMMAND });
    await (await chat.newSessionItem).click();

    await (await chat.emptyState).waitForDisplayed({ timeout: COMMAND });
    assert.strictEqual(await chat.messageCount("assistant"), 0);
  });

  it("stops the agent and starts a new generation on Start agent", async () => {
    await chat.pick(chat.menuButton, chat.stopAgentItem);
    await chat.waitForConnection(name, "stopped");
    await (await chat.stoppedState).waitForDisplayed({ timeout: COMMAND });

    await (await chat.startAgentButton).click();
    await chat.waitForConnection(name, "attached");
    await (await chat.emptyState).waitForDisplayed({ timeout: COMMAND });
  });

  it("shows the deployed state for a sandbox Mutagen reports on a remote host", async () => {
    const deployed = `e2e-chat-deployed-${Date.now()}`;
    // SandboxRouter::locate takes the machine from the sync session named after the sandbox, so a
    // scripted session on another host makes the route non-local (Resolved Decision 11).
    await configureMock("mutagen.sync-list", {
      kind: "return-value",
      stdout: JSON.stringify([{ name: deployed, alpha: { host: "" }, beta: { host: "remote-box" }, status: "watching" }]),
    });
    await invokeCmd("open_agent_chat", { name: deployed, workspacePath: path.join(WORKSPACE_ROOT, deployed) });
    await (await chat.root(deployed)).waitForExist({ timeout: PANEL_RENDER });
    await chat.waitForConnection(deployed, "deployed");
    await (await chat.deployedState).waitForDisplayed({ timeout: COMMAND });
    await invokeCmd("close_pane", { name: deployed, kind: "agentchat" });
  });
});
