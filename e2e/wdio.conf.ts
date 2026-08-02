import fs from "fs";
import os from "os";
import path from "path";
import { spawn, spawnSync, type ChildProcess } from "child_process";
import { fileURLToPath } from "url";

import {
  DEFAULT_MACHINE,
  provisionMachine,
  stopMachine,
  removeMachine,
  ensureMiseBaseImage,
  waitForConnection,
} from "./lib/podman-machine.js";
import { LOCAL_CONNECTION, REMOTE_CONNECTION } from "./lib/podman-connection.js";
import { ensureMutagen, forwardCreate, SWEEP_ORPHAN_SESSION, SWEEP_KEEP_SESSION } from "./lib/mutagen.js";

const __dirname = fileURLToPath(new URL(".", import.meta.url));

// Every machine a standard run needs answering before the first spec starts.
const REQUIRED_MACHINES = [DEFAULT_MACHINE, LOCAL_CONNECTION, REMOTE_CONNECTION];
// Short per-machine probe: these are already provisioned, so this confirms rather than waits out a
// boot. A repair pass can knock another machine down, so allow a few passes to reach a clean sweep.
const VERIFY_TIMEOUT_MS = 5000;
const VERIFY_PASSES = 3;

// MIO_REMOTE_KEEP=1  — leave remote machine running after the suite (fast iteration)
// MIO_REMOTE_REMOVE=1 — delete the remote machine's disk image entirely
const MIO_REMOTE_KEEP = process.env.MIO_REMOTE_KEEP === "1";
const MIO_REMOTE_REMOVE = process.env.MIO_REMOTE_REMOVE === "1";

// Isolated per-sandbox root so the developer's real Documents folder stays untouched. Under
// $HOME because only the user profile is auto-mounted into the podman VM. Read via
// SANDBOX_WORKSPACE_ROOT in src-tauri/src/backends/.
const WORKSPACE_ROOT = path.join(os.homedir(), ".sandbox-mio-e2e");
process.env.SANDBOX_WORKSPACE_ROOT = WORKSPACE_ROOT;

// Test-only GGUF for the `test_model` command, kept out of the shipped catalogue (see
// docs/features/local-llm/03_e2e.md). Set in process.env so the spawned app sees it too.
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

// Redirected out of APP_DATA_DIR (src-tauri/src/credentials/mod.rs `store_dir`) rather than
// cleaned in place: a real captured bundle would flip AgentManager.svelte to "Use saved login",
// breaking agent-setup.spec.ts, and clearing the developer's own bundle costs them a re-auth.
const E2E_CREDENTIALS_DIR = path.join(os.homedir(), ".sandbox-mio-e2e-appdata", "credentials");
process.env.MIO_CREDENTIALS_DIR = E2E_CREDENTIALS_DIR;

// Persisted app settings (src-tauri/src/settings/mod.rs `settings_path`, normally app_config_dir()
// = APP_DATA_DIR on Windows) are redirected for the same reason: each run must start from the Rust
// defaults, and resetting them in place would delete the release build's real settings file.
const E2E_SETTINGS_DIR = path.join(os.homedir(), ".sandbox-mio-e2e-appdata", "settings");
process.env.MIO_SETTINGS_DIR = E2E_SETTINGS_DIR;

// Kills target/debug processes left over from a previous run (they can outlive their parent,
// sync/mod.rs `shutdown`) — a stale one locks its .exe and fails the build below (LNK1104).
// Matches only target/debug paths, leaving Podman Desktop alone.
function killStaleDevProcesses(): void {
  if (process.platform !== "win32") return;
  spawnSync(
    "powershell",
    [
      "-NoProfile",
      "-Command",
      "Get-Process -Name podman,win-sshproxy,gvproxy,sandbox-mio,mutagen -ErrorAction SilentlyContinue | " +
        "Where-Object { $_.Path -and $_.Path -like '*\\target\\debug\\*' } | " +
        "Stop-Process -Force -ErrorAction SilentlyContinue",
    ],
    { stdio: "ignore" },
  );
}

let tauriDriver: ChildProcess | undefined;
let exit = false;

const ext = process.platform === "win32" ? ".exe" : "";

// Repeated or comma-separated CLI flag values in either form (`--spec a --spec b`, `--spec a,b`,
// `--spec=a,b`). The equals form must parse too, or IS_LIGHT_RUN below silently reads false.
function argValues(flag: string): string[] {
  return process.argv
    .flatMap((arg, i) => {
      if (arg === flag) return [process.argv[i + 1] ?? ""];
      return arg.startsWith(`${flag}=`) ? [arg.slice(flag.length + 1)] : [];
    })
    .flatMap((v) => v.split(","))
    .filter(Boolean);
}

// The tier is a property of *where a spec lives*, not how the run was invoked: `--suite light`,
// `--spec test/specs/light/<file>.ts`, and an ad-hoc single-file run must all get the mock, or a
// light spec silently runs against real podman and mutates the developer's host.
const isLightSpec = (spec: string) => spec.replace(/\\/g, "/").includes("/test/specs/light/");

// Launcher-side view of the tier, for the hooks that run once per *run* (onPrepare/onComplete) and
// so have no spec list to consult. Per-session hooks use `specs` instead — see beforeSession.
const requestedSpecs = argValues("--spec");
const requestedSuites = argValues("--suite");
const IS_LIGHT_RUN =
  requestedSpecs.length > 0
    ? requestedSpecs.every(isLightSpec)
    : requestedSuites.length > 0 && requestedSuites.every((s) => s === "light");

// Per tier, because onPrepare truncates it: a light run is the usual way to sanity-check a fix and
// would otherwise wipe the standard failure being investigated before it could be read.
const FAILURES_LOG = path.join(__dirname, `failures-${IS_LIGHT_RUN ? "light" : "standard"}.log`);

// `--suite light --spec <file>` does not narrow the run — @wdio/cli resolves `suites` before
// applying `--spec` (docs/features/scriptable-mock-backend/01_ideas.md Validation). MIO_SPEC
// substring-filters the light suite's glob at config-load time instead.
const MIO_SPEC = process.env.MIO_SPEC;
const LIGHT_SPECS = MIO_SPEC
  ? [`./test/specs/light/**/*${MIO_SPEC}*.ts`]
  : ["./test/specs/light/**/*.ts"];

export const config: WebdriverIO.Config = {
  hostname: "127.0.0.1",
  port: 4444,
  // Default run: standard tier only, against real podman. `light/` and `heavy/` run via their
  // own suites below (pnpm run test:light / test:heavy / test:llm). See e2e/AGENTS.md.
  specs: ["./test/specs/standard/**/*.ts"],
  suites: {
    heavy: ["./test/specs/heavy/**/*.ts"],
    light: LIGHT_SPECS,
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
    // Wiped wholesale, not cleaned in place: settings must start from Rust defaults or a prior
    // aborted run pollutes test assertions; credentials must start empty or agent-setup.spec.ts
    // sees "Use saved login" and fails.
    fs.rmSync(E2E_SETTINGS_DIR, { recursive: true, force: true });
    fs.rmSync(E2E_CREDENTIALS_DIR, { recursive: true, force: true });
    if (IS_LIGHT_RUN) {
      console.log("[e2e] light suite: skipping podman machine provisioning (MIO_MOCK_BACKEND=1).");
    } else {
      // check_podman_ready probes DEFAULT_MACHINE unconditionally regardless of PODMAN_CONNECTION,
      // so every standard spec needs it running. Abort on failure — WDIO logs onPrepare errors but
      // keeps going, and the app would otherwise start against the "Start Podman" gate.
      try {
        provisionMachine(DEFAULT_MACHINE);
      } catch (e) {
        console.error(`[e2e] ${DEFAULT_MACHINE} not ready: ${e instanceof Error ? e.message : String(e)}`);
        console.error("[e2e] Aborting suite — the startup-recovery probe targets this machine unconditionally.");
        process.exit(1);
      }
      try {
        provisionMachine(LOCAL_CONNECTION);
      } catch (e) {
        console.error(`[e2e] Local machine not ready: ${e instanceof Error ? e.message : String(e)}`);
        console.error("[e2e] Aborting suite — cannot run without the local Podman connection.");
        process.exit(1);
      }
      // Deploy target for deploy-remote.spec.ts; provisioned here, torn down in onComplete. Abort
      // on failure — WDIO logs onPrepare errors but keeps going, burning remote specs' timeouts.
      try {
        provisionMachine(REMOTE_CONNECTION);
      } catch (e) {
        console.error(`[e2e] Remote machine not ready: ${e instanceof Error ? e.message : String(e)}`);
        console.error("[e2e] Aborting suite — remote-dependent specs cannot run without the connection.");
        process.exit(1);
      }
      // A late recovery's `wsl --shutdown` stops every WSL instance, so machines provisioned first
      // aren't safe from a later one going down. Re-verify all three and repair until one clean pass.
      for (let pass = 1; ; pass++) {
        const down = REQUIRED_MACHINES.filter((name) => !waitForConnection(name, VERIFY_TIMEOUT_MS));
        if (down.length === 0) break;
        if (pass > VERIFY_PASSES) {
          console.error(`[e2e] ${down.join(", ")} still unreachable after ${VERIFY_PASSES} repair passes.`);
          console.error("[e2e] Aborting suite — a machine keeps going down as another one recovers.");
          process.exit(1);
        }
        console.warn(`[e2e] ${down.join(", ")} went down during provisioning (pass ${pass}), repairing...`);
        for (const name of down) {
          try {
            provisionMachine(name);
          } catch (e) {
            console.error(`[e2e] Could not repair ${name}: ${e instanceof Error ? e.message : String(e)}`);
            process.exit(1);
          }
        }
      }

      // Stage the mise base image on both connections before any spec runs.
      ensureMiseBaseImage(LOCAL_CONNECTION, [REMOTE_CONNECTION]);
    }
    // tauri-build has no rerun-if-changed for the regenerated ../build assets, so a frontend-only
    // change leaves cargo relinking nothing — a stale `tauri dev` exe would point at devUrl and
    // show "lost connection" under wdio. Deleting it forces a relink every run.
    killStaleDevProcesses();
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
    // --no-bundle skips Tauri `resources`, so stage llama.cpp next to the exe for resource_dir()/llama
    // (src-tauri/src/inference/llama.rs). Skipped with a warning if not staged, so only
    // local-llm.spec.ts fails rather than the whole suite. See docs/features/local-llm/03_e2e.md.
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
  beforeSession: (_config, _capabilities, specs) => {
    // Decided from the resolved spec paths, not IS_LIGHT_RUN's CLI-flag approximation: this hook
    // hands MIO_MOCK_BACKEND to the app, so a light spec must never reach a real podman.
    const isLightSession = specs.length > 0 && specs.every(isLightSpec);
    // Seeds forward-sweep's preconditions before the app spawns (its startup sweep is what's under
    // test): an orphan and a bystander session in the app's own Mutagen daemon. Local tcp endpoints
    // keep session creation instant and SSH-free — the sweep matches by name only.
    if (!isLightSession && specs.some((s) => s.includes("forward-sweep"))) {
      ensureMutagen();
      forwardCreate(SWEEP_ORPHAN_SESSION, "tcp:127.0.0.1:38471", "tcp:127.0.0.1:1");
      forwardCreate(SWEEP_KEEP_SESSION, "tcp:127.0.0.1:38473", "tcp:127.0.0.1:1");
    }
    tauriDriver = spawn(
      path.resolve(os.homedir(), ".cargo", "bin", `tauri-driver${ext}`),
      ["--native-driver", "../tools/msedgedriver.exe"],
      {
        stdio: [null, process.stdout, process.stderr],
        env: {
          ...process.env,
          PODMAN_CONNECTION: LOCAL_CONNECTION,
          ...(isLightSession ? { MIO_MOCK_BACKEND: "1" } : {}),
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
  // One merged chrome webview hosts the whole app UI (docs/features/consolidate-app-container/02_dev.md);
  // each Terminal/AgentWeb pane opens its own overlay webview. Pin the session to the app UI
  // handle so it doesn't land on a pane's window instead.
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
    if (!IS_LIGHT_RUN) {
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
