import fs from "fs";
import os from "os";
import path from "path";
import { spawn, spawnSync, type ChildProcess } from "child_process";
import { fileURLToPath } from "url";

import { provisionMachine, stopMachine, removeMachine, ensureMiseBaseImage } from "./lib/podman-machine.js";
import { LOCAL_CONNECTION, REMOTE_CONNECTION } from "./lib/podman-connection.js";

const __dirname = fileURLToPath(new URL(".", import.meta.url));
const FAILURES_LOG = path.join(__dirname, "failures.log");

// MIO_REMOTE_KEEP=1  — leave remote machine running after the suite (fast iteration)
// MIO_REMOTE_REMOVE=1 — delete the remote machine's disk image entirely
const MIO_REMOTE_KEEP = process.env.MIO_REMOTE_KEEP === "1";
const MIO_REMOTE_REMOVE = process.env.MIO_REMOTE_REMOVE === "1";

// Isolated workspace root for the app's per-sandbox directories. Set so specs know where the
// app writes them and the developer's real Documents folder is left untouched. Kept under
// $HOME because the podman machine only auto-mounts the user profile into its VM, which the
// bind mount needs. Passed to the app process via SANDBOX_WORKSPACE_ROOT (read in
// src-tauri/src/backends/) and read back by the workspace spec.
const WORKSPACE_ROOT = path.join(os.homedir(), ".sandbox-mio-e2e");
process.env.SANDBOX_WORKSPACE_ROOT = WORKSPACE_ROOT;

let tauriDriver: ChildProcess | undefined;
let exit = false;

const ext = process.platform === "win32" ? ".exe" : "";

export const config: WebdriverIO.Config = {
  hostname: "127.0.0.1",
  port: 4444,
  specs: ["./test/specs/**/*.ts"],
  maxInstances: 1,
  capabilities: [
    {
      maxInstances: 1,
      "tauri:options": {
        application: path.resolve(__dirname, `../src-tauri/target/debug/sandbox-mio${ext}`),
      },
    } as WebdriverIO.Capabilities,
  ],
  reporters: ["spec"],
  framework: "mocha",
  mochaOpts: {
    ui: "bdd",
    // 3 min: deploy involves an image save/load + Mutagen session startup, which can run long.
    timeout: 180000,
  },
  onPrepare: () => {
    fs.writeFileSync(FAILURES_LOG, "");
    fs.rmSync(WORKSPACE_ROOT, { recursive: true, force: true });
    // Abort the entire suite on failure: WDIO catches onPrepare errors and logs "Error in hook"
    // but continues running, so the app would start against a stopped machine.
    try {
      provisionMachine(LOCAL_CONNECTION);
    } catch (e) {
      console.error(`[e2e] Local machine not ready: ${e instanceof Error ? e.message : String(e)}`);
      console.error("[e2e] Aborting suite — cannot run without the local Podman connection.");
      process.exit(1);
    }
    // The remote machine is used as a deploy target by deploy-remote.spec.ts. Provisioned here
    // so the spec can assume it is up; cleaned up in onComplete.
    // Abort the entire suite on failure: WDIO catches onPrepare errors and logs "Error in hook"
    // but continues running, so remote-dependent specs would silently burn their full timeouts.
    try {
      provisionMachine(REMOTE_CONNECTION);
    } catch (e) {
      console.error(`[e2e] Remote machine not ready: ${e instanceof Error ? e.message : String(e)}`);
      console.error("[e2e] Aborting suite — remote-dependent specs cannot run without the connection.");
      process.exit(1);
    }
    // Stage the mise base image on both connections before any spec runs.
    ensureMiseBaseImage(LOCAL_CONNECTION, [REMOTE_CONNECTION]);
    spawnSync("pnpm", ["tauri", "build", "--debug", "--no-bundle"], {
      cwd: path.resolve(__dirname, ".."),
      stdio: "inherit",
      shell: true,
    });
    // Mutagen needs its agent bundle adjacent to the sidecar binary to install agents on new SSH
    // endpoints. The bundle is in tools/ but Tauri's build does not copy it to target/debug/.
    fs.copyFileSync(
      path.resolve(__dirname, "../tools/mutagen-agents.tar.gz"),
      path.resolve(__dirname, "../src-tauri/target/debug/mutagen-agents.tar.gz"),
    );
  },
  beforeSession: () => {
    tauriDriver = spawn(
      path.resolve(os.homedir(), ".cargo", "bin", `tauri-driver${ext}`),
      ["--native-driver", "../tools/msedgedriver.exe"],
      {
        stdio: [null, process.stdout, process.stderr],
        env: { ...process.env, PODMAN_CONNECTION: LOCAL_CONNECTION },
      }
    );
    tauriDriver.on("error", (error) => {
      console.error("tauri-driver error:", error);
      process.exit(1);
    });
    tauriDriver.on("exit", (code) => {
      if (!exit) {
        console.error("tauri-driver exited with code:", code);
        process.exit(1);
      }
    });
  },
  afterTest(_test, _context, result) {
    if (!result.passed && result.error) {
      const err = result.error as Error & { stack?: string };
      const stripAnsi = (s: string) => s.replace(/\x1B\[[0-9;]*[A-Za-z]/g, "");
      const title = _test.fullTitle || `${_test.parent} > ${_test.title}`;
      const message = stripAnsi(err.message ?? String(err));
      const stack = stripAnsi(err.stack ?? "");
      const lines = [`FAILED: ${title}`, message, stack, ""].join("\n");
      fs.appendFileSync(FAILURES_LOG, lines);
    }
  },
  afterSession: () => {
    closeTauriDriver();
  },
  onComplete: () => {
    fs.rmSync(WORKSPACE_ROOT, { recursive: true, force: true });

    // Remove the SSH alias block the app's deploy flow wrote under ~/.ssh/sandbox-mio/config
    // and strip the one-time Include line from ~/.ssh/config so the user's file is left clean.
    const sshDir = path.join(os.homedir(), ".ssh");
    const managedConfig = path.join(sshDir, "sandbox-mio", "config");
    fs.rmSync(managedConfig, { force: true });
    const userConfig = path.join(sshDir, "config");
    if (fs.existsSync(userConfig)) {
      const cleaned = fs
        .readFileSync(userConfig, "utf8")
        .split(/\r?\n/)
        .filter((l) => l.trim() !== "Include sandbox-mio/config")
        .join("\n")
        .replace(/\n{3,}/g, "\n\n")
        .trimEnd();
      fs.writeFileSync(userConfig, cleaned ? `${cleaned}\n` : "", "ascii");
    }

    // Tear down the remote machine unless the caller wants to keep it.
    if (MIO_REMOTE_REMOVE) removeMachine(REMOTE_CONNECTION);
    else if (!MIO_REMOTE_KEEP) stopMachine(REMOTE_CONNECTION);
  },
};

function closeTauriDriver(): void {
  exit = true;
  tauriDriver?.kill();
}

function onShutdown(fn: () => void): void {
  const cleanup = () => {
    try {
      fn();
    } finally {
      process.exit();
    }
  };
  process.on("exit", cleanup);
  process.on("SIGINT", cleanup);
  process.on("SIGTERM", cleanup);
  process.on("SIGHUP", cleanup);
  process.on("SIGBREAK", cleanup);
}

onShutdown(() => {
  closeTauriDriver();
});
