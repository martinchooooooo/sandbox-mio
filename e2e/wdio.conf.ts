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

// Tiny test-only GGUF the /models route offers via the `test_model` command (kept out of the
// shipped catalogue so a ~100 MB model never leaks into the product). Values from the 2026-06-29
// bring-up spike (docs/features/local-llm/03_e2e.md). Set in process.env so both this spec process
// and the app (spawned with `...process.env` in beforeSession) see it.
const TEST_MODEL = {
  id: "smollm2-135m",
  label: "SmolLM2 135M (test)",
  repo: "bartowski/SmolLM2-135M-Instruct-GGUF",
  file: "SmolLM2-135M-Instruct-Q4_K_M.gguf",
  sha256: "2e8040ceae7815abe0dcb3540b9995eaa1fa0d2ca9e797d0a635ae4433c68c2d",
  sizeBytes: 105454432,
};
process.env.MIO_TEST_MODEL = JSON.stringify(TEST_MODEL);

// The debug build writes models under app_data_dir() = %APPDATA%/<bundle identifier>/models
// (identifier com.tincho.sandbox-mio in src-tauri/tauri.conf.json). Only the test model's own
// subdirectory is cleaned up, never the whole store, since debug and release share the identifier.
const APP_DATA_DIR = path.join(
  process.env.APPDATA ?? path.join(os.homedir(), "AppData", "Roaming"),
  "com.tincho.sandbox-mio",
);
const APP_DATA_MODELS = path.join(APP_DATA_DIR, "models");

let tauriDriver: ChildProcess | undefined;
let exit = false;

const ext = process.platform === "win32" ? ".exe" : "";

// True when invoked as `wdio run wdio.conf.ts --suite light` (see package.json's `test:light`).
// The light suite runs against the in-memory mock backend (MIO_MOCK_BACKEND=1, see
// src-tauri/src/backends/mod.rs), so onPrepare skips podman provisioning and beforeSession skips
// pinning a real connection.
const suiteFlagIndex = process.argv.indexOf("--suite");
const IS_LIGHT_SUITE = suiteFlagIndex !== -1 && process.argv[suiteFlagIndex + 1] === "light";

export const config: WebdriverIO.Config = {
  hostname: "127.0.0.1",
  port: 4444,
  // Default run: the standard tier only. `light/` (mocked, no podman) and `heavy/` (large-asset
  // downloads) are excluded here and run via their own suites below — `pnpm run test:light`,
  // `pnpm run test:heavy`, or `pnpm run test:llm` for just the local-llm spec. See e2e/AGENTS.md.
  specs: ["./test/specs/standard/*.ts"],
  suites: {
    heavy: ["./test/specs/heavy/**/*.ts"],
    light: ["./test/specs/light/**/*.ts"],
  },
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
    // Reset persisted app settings so each run starts from the Rust defaults (inferenceEngine:
    // "llama"). Without this, a previous aborted run that saved "foundry" would pollute test 3's
    // Start-gate assertion (the gate only fires when engine === "llama").
    fs.rmSync(path.join(APP_DATA_DIR, "settings.json"), { force: true });
    if (IS_LIGHT_SUITE) {
      console.log("[e2e] light suite: skipping podman machine provisioning (MIO_MOCK_BACKEND=1).");
    } else {
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
      // Guard: podman-startup-recovery.spec.ts needs podman-machine-default to be initialized (its
      // own before() hook stops it but cannot init it). Warn (don't abort) so the rest of the suite
      // still runs on machines where only mio-tests is provisioned — the spec then fails on Step 2
      // navigation with a clear timeoutMsg instead of here.
      const defaultMachineRaw = spawnSync(
        "podman", ["machine", "list", "--format", "json"],
        { encoding: "utf8", shell: true },
      );
      const defaultMachines: Array<{ Name: string }> = JSON.parse(defaultMachineRaw.stdout || "[]");
      if (!defaultMachines.some((m) => m.Name === "podman-machine-default")) {
        console.warn(
          "[e2e] podman-machine-default is not initialized. " +
            "podman-startup-recovery.spec.ts will fail on navigation. " +
            "Run `podman machine init && podman machine start` once to initialize it.",
        );
      }
    }
    // Delete the existing debug exe before building. `tauri build` runs beforeBuildCommand
    // (pnpm build) which regenerates ../build, but tauri-build does not emit a rerun-if-changed
    // for those asset contents — so when only the frontend changed, cargo sees no Rust input
    // change and skips relinking the bin, leaving whatever exe was last linked. If that leftover
    // is a `tauri dev` binary, its webview points at devUrl (localhost:1420) and shows a
    // "lost connection" page under wdio. Removing the output forces cargo to relink and re-embed
    // the current assets every run.
    const appExe = path.resolve(__dirname, `../src-tauri/target/debug/sandbox-mio${ext}`);
    fs.rmSync(appExe, { force: true });
    // Abort on a failed build: spawnSync does not throw on a non-zero exit, and a silent failure
    // would leave no exe (or a stale one) for tauri-driver to launch.
    const build = spawnSync("pnpm", ["tauri", "build", "--debug", "--no-bundle"], {
      cwd: path.resolve(__dirname, ".."),
      stdio: "inherit",
      shell: true,
    });
    if (build.status !== 0 || !fs.existsSync(appExe)) {
      console.error(`[e2e] tauri build --debug failed (exit ${build.status}); ${appExe} not produced.`);
      console.error("[e2e] Aborting suite — cannot run specs without a freshly built app binary.");
      process.exit(1);
    }
    // Mutagen needs its agent bundle adjacent to the sidecar binary to install agents on new SSH
    // endpoints. The bundle is in tools/ but Tauri's build does not copy it to target/debug/.
    fs.copyFileSync(
      path.resolve(__dirname, "../tools/mutagen-agents.tar.gz"),
      path.resolve(__dirname, "../src-tauri/target/debug/mutagen-agents.tar.gz"),
    );
    // Stage the bundled llama.cpp Vulkan engine next to the debug exe so resource_dir()/llama
    // resolves: the --no-bundle build does not copy Tauri `resources`, and the engine launches
    // llama-server from <resource_dir>/llama (src-tauri/src/inference/llama.rs). Mirrors the
    // Mutagen sidecar staging requirement. Skipped with a warning when the binaries are not staged
    // in the repo, so the rest of the suite still runs (local-llm.spec.ts then fails at sidecar
    // resolution). See docs/features/local-llm/03_e2e.md.
    const llamaSrc = path.resolve(__dirname, "../src-tauri/binaries/llama");
    const llamaDst = path.resolve(__dirname, "../src-tauri/target/debug/llama");
    if (fs.existsSync(path.join(llamaSrc, `llama-server${ext}`))) {
      fs.cpSync(llamaSrc, llamaDst, { recursive: true });
    } else {
      console.warn(
        `[e2e] llama-server not staged at ${llamaSrc}; local-llm.spec.ts will fail at sidecar ` +
          "resolution. Stage the llama.cpp Vulkan release there per docs/features/local-llm/03_e2e.md.",
      );
    }
  },
  beforeSession: () => {
    tauriDriver = spawn(
      path.resolve(os.homedir(), ".cargo", "bin", `tauri-driver${ext}`),
      ["--native-driver", "../tools/msedgedriver.exe"],
      {
        stdio: [null, process.stdout, process.stderr],
        env: {
          ...process.env,
          PODMAN_CONNECTION: LOCAL_CONNECTION,
          ...(IS_LIGHT_SUITE ? { MIO_MOCK_BACKEND: "1" } : {}),
        },
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
  // The main window is a multi-webview container (docs/features/embedded-windows/02_dev.md):
  // the app UI lives in the `main-global` child webview, next to the `main-switcher` tab bar
  // (and, once opened, per-sandbox terminal webviews). Each webview is a WebDriver window
  // handle and the session may attach to any of them, so pin the session to the app UI handle —
  // the one whose page is neither the /workspace/* chrome nor a remote terminal origin.
  before: async () => {
    for (const handle of await browser.getWindowHandles()) {
      await browser.switchToWindow(handle);
      const pagePath = await browser.execute(() => window.location.pathname);
      if (!pagePath.startsWith("/workspace")) return;
    }
    throw new Error("[e2e] no app webview found among the window handles");
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

    // Remove the downloaded test GGUF so it does not persist or grow the disk across runs. Only
    // the test model's own subdirectory is deleted (debug and release share the app-data store).
    fs.rmSync(path.join(APP_DATA_MODELS, TEST_MODEL.id), { recursive: true, force: true });

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

    // Tear down the remote machine unless the caller wants to keep it. Never provisioned for the
    // light suite, so there is nothing to tear down.
    if (!IS_LIGHT_SUITE) {
      if (MIO_REMOTE_REMOVE) removeMachine(REMOTE_CONNECTION);
      else if (!MIO_REMOTE_KEEP) stopMachine(REMOTE_CONNECTION);
    }
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
