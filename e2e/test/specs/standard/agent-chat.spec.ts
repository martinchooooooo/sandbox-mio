import assert from "node:assert";
import { spawn, type ChildProcess } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import readline from "node:readline";
import { fileURLToPath } from "node:url";

import ProjectsPage from "../../pageobjects/projects.page.js";
import workspaceDashboard from "../../pageobjects/workspace-dashboard.page.js";
import chat from "../../pageobjects/agent-chat.page.js";
import { captureDiagOnFailure } from "../../../lib/diag.js";
import { useSandboxWorkspace } from "../../../lib/sandbox-suite.js";
import { podman, podmanOk, LOCAL_CONNECTION } from "../../../lib/podman-connection.js";
import { workspaceState, paneFor, closePane } from "../../../lib/workspace.js";
import { AGENT_INSTALL, AGENT_SETTLE, COMMAND, PANEL_RENDER, SUBPROCESS } from "../../../lib/timeouts.js";

// Spec S1 of docs/features/native-agent-ui/03_e2e.md: the real supervisor, writer lock and pi in a
// Podman sandbox. No model is served, so every pi run ends in a refused model connection.

/** Mirrors SCRIPT_DIR in src-tauri/src/agent_host/mod.rs. */
const SCRIPT_DIR = "/usr/local/lib/mio";
/** Mirrors STATE in src-tauri/resources/agent-host/pi-host.sh. */
const STATE = "/run/mio/pi-host";
const FIXTURE = fileURLToPath(new URL("../../../fixtures/pi-extensions/spikeconfirm.ts", import.meta.url));

interface AttachHeader {
  type: string;
  generation: string;
  offset: number;
  writer: boolean;
}

/** A spec-side client of `pi-attach.sh`, standing in for a second app window. */
interface SpecAttach {
  child: ChildProcess;
  header: Promise<AttachHeader>;
  /** Resolves when the in-sandbox attach ends its stream. */
  ended: Promise<void>;
  lines: string[];
}

function withTimeout<T>(promise: Promise<T>, ms: number, what: string): Promise<T> {
  return Promise.race([
    promise,
    new Promise<T>((_, reject) => setTimeout(() => reject(new Error(`${what} within ${ms}ms`)), ms)),
  ]);
}

/** The JSON records of a JSON-lines file; skips a line pi is still writing. */
function lines(text: string): Record<string, unknown>[] {
  return text.split("\n").flatMap((l) => {
    try {
      return [JSON.parse(l) as Record<string, unknown>];
    } catch {
      return [];
    }
  });
}

function jsonlFiles(dir: string): string[] {
  if (!fs.existsSync(dir)) return [];
  return fs
    .readdirSync(dir, { recursive: true, encoding: "utf8" })
    .filter((f) => f.endsWith(".jsonl"))
    .map((f) => path.join(dir, f));
}

describe("Native agent chat — real pi supervisor on Podman", function () {
  this.timeout(900000);

  const { projectName, row, workspaceDir } = useSandboxWorkspace("e2e-agentchat");
  const piAgentDir = path.join(workspaceDir, ".mio", "pi-agent");
  const runStart = Date.now();
  const spawned: ChildProcess[] = [];

  captureDiagOnFailure();

  const sandboxSh = (script: string, check = true): string =>
    podman(LOCAL_CONNECTION, ["exec", projectName, "sh", "-c", script], { check });
  const stateFile = (file: string): string => sandboxSh(`cat ${STATE}/${file}`);
  const processArgs = (): string[] => sandboxSh("ps -eo args=").split("\n").map((l) => l.trim());

  function spawnAttach(): SpecAttach {
    const child = spawn(
      "podman",
      ["--connection", LOCAL_CONNECTION, "exec", "-i", projectName, "sh", `${SCRIPT_DIR}/pi-attach.sh`, "-", "end"],
      { stdio: ["pipe", "pipe", "ignore"], shell: false },
    );
    spawned.push(child);
    const seen: string[] = [];
    const reader = readline.createInterface({ input: child.stdout! });
    const header = new Promise<AttachHeader>((resolve, reject) => {
      reader.once("line", (line) => {
        try {
          resolve(JSON.parse(line) as AttachHeader);
        } catch {
          reject(new Error(`pi-attach.sh printed a non-JSON first line: ${line}`));
        }
        reader.on("line", (l) => seen.push(l));
      });
      child.once("exit", (code) => reject(new Error(`pi-attach.sh exited with ${code} before its header`)));
    });
    const ended = new Promise<void>((resolve) => child.stdout!.once("close", () => resolve()));
    return { child, header, ended, lines: seen };
  }

  async function kill(attach: SpecAttach): Promise<void> {
    if (attach.child.exitCode !== null || attach.child.signalCode !== null) return;
    const exited = new Promise<void>((resolve) => attach.child.once("exit", () => resolve()));
    attach.child.kill();
    await withTimeout(exited, SUBPROCESS, "the spec-side podman exec did not exit");
  }

  async function waitForInSandbox(predicate: () => boolean, timeoutMsg: string): Promise<void> {
    await browser.waitUntil(async () => predicate(), { timeout: SUBPROCESS, interval: 1000, timeoutMsg });
  }

  let reader: SpecAttach;
  let piPid: string;
  let generation: string;

  after(async () => {
    // killStaleDevProcesses in wdio.conf.ts matches only the worktree's target\debug\ processes.
    for (const child of spawned) if (child.exitCode === null && child.signalCode === null) child.kill();
    await closePane(projectName, "agentchat").catch(() => undefined);
  });

  it("creates and starts a sandbox and installs the pinned pi", async () => {
    await ProjectsPage.open();
    await ProjectsPage.createProject(projectName, { timeout: 600000 });
    await row.start();
    await row.waitForRunning();
    await row.openWorkspace();
    await workspaceDashboard.waitForRendered(projectName);

    // LAUNCHABLE_APPS offers the chat only once pi is installed (src/lib/workspace/apps.ts).
    assert(
      !(await (await workspaceDashboard.launcherButton("agentchat")).isExisting()),
      "The Chat launcher is offered before pi is installed.",
    );

    await workspaceDashboard.startAgentInstall("Pi");
    await workspaceDashboard.waitForAgentRow("Pi", AGENT_INSTALL);

    const version = podman(LOCAL_CONNECTION, ["exec", "-w", "/root/work", projectName, "mise", "exec", "--", "pi", "--version"]);
    assert.match(version, /\b1\.5\.0\b/, `pi --version printed "${version}"; the catalogue pins pi@1.5.0 (PI_PINNED_VERSION).`);

    // Provisioning runs after the install; pi reads PI_CODING_AGENT_DIR from mise.toml at start.
    await browser.waitUntil(
      async () =>
        fs.existsSync(path.join(piAgentDir, "models.json")) &&
        fs.readFileSync(path.join(workspaceDir, "mise.toml"), "utf8").includes("PI_CODING_AGENT_DIR"),
      { timeout: COMMAND, timeoutMsg: "pi was never provisioned: no models.json or no PI_CODING_AGENT_DIR in mise.toml" },
    );
  });

  it("seeds the spikeconfirm extension before the first attach", () => {
    const extensions = path.join(piAgentDir, "extensions");
    fs.mkdirSync(extensions, { recursive: true });
    fs.copyFileSync(FIXTURE, path.join(extensions, "spikeconfirm.ts"));
  });

  it("offers the Chat launcher and opens a webview-less chat pane", async () => {
    const launcher = await workspaceDashboard.launcherButton("agentchat");
    await launcher.waitForDisplayed({ timeout: PANEL_RENDER, timeoutMsg: "the Chat launcher never appeared with pi installed" });
    await workspaceDashboard.launchApp("agentchat");

    await browser.waitUntil(async () => paneFor(await workspaceState(), projectName, "agentchat") !== undefined, {
      timeout: COMMAND,
      timeoutMsg: "no agentchat pane appeared after clicking the Chat launcher",
    });
    const state = await workspaceState();
    const pane = paneFor(state, projectName, "agentchat")!;
    assert.strictEqual(pane.forward_name, null, "a chat pane must carry no forward (Resolved Decision 14)");
    assert.strictEqual(pane.url, null, "a chat pane must carry no URL (Resolved Decision 14)");
    assert.strictEqual(state.active_app, pane.label, "the new chat pane did not activate");
  });

  it("installs the supervisor scripts and starts pi --mode rpc as the writer", async () => {
    await (await chat.root(projectName)).waitForExist({ timeout: PANEL_RENDER });
    await chat.waitForConnection(projectName, "attached", SUBPROCESS);
    assert.strictEqual(await chat.attribute(projectName, "data-writer"), "true");

    for (const script of ["pi-host.sh", "pi-attach.sh"]) {
      const body = sandboxSh(`cat ${SCRIPT_DIR}/${script}`);
      assert(body.startsWith("#!/bin/sh"), `${SCRIPT_DIR}/${script} is missing or empty`);
      assert(!body.includes("\r"), `${SCRIPT_DIR}/${script} has CRLF line endings, which break sh`);
    }
    const files = sandboxSh(`ls ${STATE}`).split(/\s+/);
    for (const f of ["in.fifo", "journal.jsonl", "input.jsonl", "generation", "pi.pid", "keeper.pid"]) {
      assert(files.includes(f), `${STATE} holds no ${f}; it holds: ${files.join(" ")}`);
    }
    piPid = stateFile("pi.pid");
    generation = stateFile("generation");
    const cmdline = sandboxSh(`tr '\\0' ' ' < /proc/${piPid}/cmdline`);
    assert(
      cmdline.includes("--mode rpc") && cmdline.includes("--session-dir"),
      `pi.pid ${piPid} runs "${cmdline}", expected pi --mode rpc --session-dir`,
    );

    await browser.waitUntil(async () => ((await (await chat.modelButton).getAttribute("data-chat-model")) ?? "") !== "", {
      timeout: COMMAND,
      timeoutMsg: "the model selector stayed empty; get_state reported no model from the provisioned models.json",
    });
  });

  it("attaches a second client as a reader while the app holds the writer lock", async () => {
    reader = spawnAttach();
    const header = await withTimeout(reader.header, SUBPROCESS, "the spec-side attach printed no header");
    assert.strictEqual(header.type, "mio_attach");
    assert.strictEqual(header.writer, false, "a second attach became the writer while the app held writer.lock");
    assert.strictEqual(header.generation, generation);
  });

  it("raises a dialog through the real pipe and logs the answer in input.jsonl", async () => {
    await chat.send("/spikeconfirm");
    const dialog = await chat.dialog("confirm");
    await dialog.waitForDisplayed({ timeout: PANEL_RENDER, timeoutMsg: "no confirm dialog; is the spikeconfirm extension loaded?" });
    const id = await dialog.getAttribute("data-dialog-id");
    assert(id, "the dialog carries no request id");

    const requests = lines(stateFile("journal.jsonl")).filter((r) => r.type === "extension_ui_request");
    assert(
      requests.some((r) => r.id === id && r.method === "confirm"),
      `journal.jsonl holds no confirm extension_ui_request with id ${id}`,
    );
    await browser.waitUntil(async () => reader.lines.some((l) => l.includes(id)), {
      timeout: COMMAND,
      timeoutMsg: "the spec-side reader never received the dialog request",
    });

    const no = await dialog.$("[data-dialog-no]");
    await no.waitForClickable({ timeout: COMMAND });
    await no.click();
    await browser.waitUntil(async () => (await chat.dialogCount()) === 0, {
      timeout: COMMAND,
      timeoutMsg: "the answered dialog stayed on screen",
    });
    await (await chat.toast).waitForDisplayed({ timeout: PANEL_RENDER });

    const answer = lines(stateFile("input.jsonl")).find((r) => r.type === "extension_ui_response" && r.id === id);
    assert(answer, `input.jsonl holds no extension_ui_response for ${id}`);
    assert.strictEqual(answer.confirmed, false, `the logged answer is ${JSON.stringify(answer)}, expected confirmed: false`);
    await waitForInSandbox(
      () =>
        lines(stateFile("journal.jsonl")).some(
          (r) => r.method === "notify" && String(r.message).includes("answered: false"),
        ),
      "pi never notified the answer, so the response did not reach pi",
    );
  });

  it("closing the chat tab detaches without stopping pi", async () => {
    await kill(reader);
    await (await chat.closeTabButton(projectName)).click();
    await browser.waitUntil(async () => paneFor(await workspaceState(), projectName, "agentchat") === undefined, {
      timeout: COMMAND,
      timeoutMsg: "the agentchat pane survived its close button",
    });

    await waitForInSandbox(
      () => !processArgs().some((a) => a.includes("pi-attach.sh")),
      "a pi-attach.sh process outlived both the closed tab and the killed reader",
    );
    assert(
      podmanOk(LOCAL_CONNECTION, ["exec", projectName, "flock", "-n", `${STATE}/writer.lock`, "true"]),
      "writer.lock is still held after every attach ended",
    );
    assert.strictEqual(stateFile("pi.pid"), piPid, "closing the tab restarted pi");
    assert(podmanOk(LOCAL_CONNECTION, ["exec", projectName, "kill", "-0", piPid]), "closing the tab stopped pi");
  });

  it("an earlier client keeps the lock until Take control ends its stream", async () => {
    const writer = spawnAttach();
    const header = await withTimeout(writer.header, SUBPROCESS, "the spec-side attach printed no header");
    assert.strictEqual(header.writer, true, "the first attach after every client left did not become the writer");

    await workspaceDashboard.waitForRendered(projectName);
    await workspaceDashboard.launchApp("agentchat");
    await (await chat.root(projectName)).waitForExist({ timeout: PANEL_RENDER });
    await chat.waitForConnection(projectName, "attached", SUBPROCESS);
    assert.strictEqual(await chat.attribute(projectName, "data-writer"), "false");
    await (await chat.readonlyBanner).waitForDisplayed({ timeout: COMMAND });
    assert.strictEqual(await (await chat.input).isEnabled(), false, "the composer accepts input while read-only");

    const take = await chat.takeControlButton;
    await take.waitForClickable({ timeout: COMMAND });
    await take.click();
    await browser.waitUntil(async () => (await chat.attribute(projectName, "data-writer")) === "true", {
      timeout: SUBPROCESS,
      timeoutMsg: "the chat never became the writer after Take control",
    });
    await (await chat.readonlyBanner).waitForExist({ timeout: COMMAND, reverse: true });
    await withTimeout(writer.ended, SUBPROCESS, "the replaced writer's stream did not end");
  });

  it("shows the inference-down banner when no model is served", async () => {
    await chat.send("hello");
    // pi reports the refused connection as an errored assistant message, retries three times,
    // then settles (docs/features/native-agent-ui/03_e2e.md, De-risking Spike Results).
    await (await chat.inferenceDownBanner).waitForDisplayed({
      timeout: AGENT_SETTLE,
      timeoutMsg: "no inference-down banner after a prompt with no model served",
    });
    await chat.waitForIdle(projectName, COMMAND);
    assert(await (await $("[data-notice='error']")).isExisting(), "the failed model request shows no error notice");
    assert(
      lines(stateFile("journal.jsonl")).some(
        (r) => r.type === "message_end" && (r.message as { stopReason?: string } | undefined)?.stopReason === "error",
      ),
      "journal.jsonl holds no assistant message_end with stopReason error",
    );
  });

  it("lists the chat's own session files and leaves pi's default session directory untouched", async () => {
    const onDisk = new Set(
      jsonlFiles(path.join(piAgentDir, "chat-sessions")).map((f) => {
        const first = fs.readFileSync(f, "utf8").split("\n")[0];
        return (JSON.parse(first) as { id?: string }).id ?? "";
      }),
    );
    assert(onDisk.size > 0, "pi wrote no session file under .mio/pi-agent/chat-sessions/");

    await (await chat.sessionsButton).click();
    await browser.waitUntil(async () => (await chat.sessionIds()).length > 0, {
      timeout: COMMAND,
      timeoutMsg: "the sessions dropdown lists no session",
    });
    const listed = await chat.sessionIds();
    await browser.keys("Escape");
    assert(
      listed.some((id) => onDisk.has(id)),
      `listed sessions ${JSON.stringify(listed)} share no id with the files on disk ${JSON.stringify([...onDisk])}`,
    );

    // Resolved Decision 8: the chat never writes into the directory Paseo's pi uses.
    const touched = jsonlFiles(path.join(piAgentDir, "sessions")).filter((f) => fs.statSync(f).mtimeMs >= runStart);
    assert.deepStrictEqual(touched, [], "the chat wrote into .mio/pi-agent/sessions/");
  });

  it("Stop agent ends pi and its keeper; Start agent begins a new generation", async () => {
    const keeperPid = stateFile("keeper.pid");
    await chat.pick(chat.menuButton, chat.stopAgentItem);
    await chat.waitForConnection(projectName, "stopped", SUBPROCESS);
    await (await chat.stoppedState).waitForDisplayed({ timeout: COMMAND });

    const args = processArgs();
    assert(!args.some((a) => a.includes("--mode rpc")), `pi survived Stop agent:\n${args.join("\n")}`);
    assert(!podmanOk(LOCAL_CONNECTION, ["exec", projectName, "kill", "-0", keeperPid]), "the keeper survived Stop agent");
    assert(!args.some((a) => a.includes("pi-attach.sh")), `an attach survived Stop agent:\n${args.join("\n")}`);

    await (await chat.startAgentButton).click();
    await chat.waitForConnection(projectName, "attached", SUBPROCESS);
    const next = stateFile("generation");
    assert.notStrictEqual(next, generation, "Start agent reused the stopped generation");
    generation = next;
  });

  it("a sandbox restart starts a new generation despite the stale pi.pid", async () => {
    piPid = stateFile("pi.pid");
    await row.stop();
    await row.waitForStopped();
    // The open chat's stream ends with the sandbox, so it offers Reconnect.
    await chat.waitForConnection(projectName, "disconnected", SUBPROCESS);
    await (await $("[data-chat-disconnected]")).waitForDisplayed({ timeout: COMMAND });

    await row.start();
    await row.waitForRunning();
    // /run is not a tmpfs, so the old pid file survives and only the cmdline check rejects it.
    assert.strictEqual(sandboxSh(`cat ${STATE}/pi.pid`, false), piPid, "pi.pid did not survive the restart");
    const reconnect = await $("[data-chat-reconnect]");
    await reconnect.waitForClickable({ timeout: COMMAND });
    await reconnect.click();
    await chat.waitForConnection(projectName, "attached", SUBPROCESS);
    assert.strictEqual(await chat.attribute(projectName, "data-writer"), "true");

    const next = stateFile("generation");
    assert.notStrictEqual(next, generation, "the restarted sandbox reattached to the old generation");
    const cmdline = sandboxSh(`tr '\\0' ' ' < /proc/${stateFile("pi.pid")}/cmdline`);
    assert(cmdline.includes("--mode rpc"), `the new pi.pid runs "${cmdline}"`);
  });
});
