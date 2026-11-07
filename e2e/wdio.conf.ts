import fs from "fs";
import net from "net";
import os from "os";
import path from "path";
import { spawn, spawnSync, type ChildProcess } from "child_process";
import { fileURLToPath } from "url";

import {
  DEFAULT_MACHINE,
  provisionMachine,
  ensureMachineInitialized,
  stopMachine,
  removeMachine,
  ensureMiseBaseImage,
  waitForConnection,
} from "./lib/podman-machine.js";
import { LOCAL_CONNECTION, REMOTE_CONNECTION } from "./lib/podman-connection.js";
import {
  ensureMutagen,
  forwardCreate,
  listMutagenDaemons,
  SWEEP_ORPHAN_SESSION,
  SWEEP_KEEP_SESSION,
} from "./lib/mutagen.js";
import {
  RUN_SLUG,
  APP_DATA_ROOT,
  WORKSPACE_ROOT,
  DEBUG_DIR,
  IS_LAUNCHER,
  acquireWorktreeLock,
  releaseWorktreeLock,
  acquireHostLock,
  releaseHostLock,
  allocateDriverPort,
  PORT_WINDOW_BASE,
  PORT_WINDOW_INDEX,
  MIO_PORT_BASE,
  MSB_HOME,
} from "./lib/run-identity.js";

const __dirname = fileURLToPath(new URL(".", import.meta.url));

// Every machine a standard run needs answering before the first spec starts. DEFAULT_MACHINE is
// excluded: onPrepare only initialises it, never starts it (see the comment there).
const REQUIRED_MACHINES = [LOCAL_CONNECTION, REMOTE_CONNECTION];
// Short per-machine probe: these are already provisioned, so this confirms rather than waits out a
// boot. A repair pass can knock another machine down, so allow a few passes to reach a clean sweep.
const VERIFY_TIMEOUT_MS = 5000;
const VERIFY_PASSES = 3;

// MIO_REMOTE_KEEP=1  — leave remote machine running after the suite (fast iteration)
// MIO_REMOTE_REMOVE=1 — delete the remote machine's disk image entirely
const MIO_REMOTE_KEEP = process.env.MIO_REMOTE_KEEP === "1";
const MIO_REMOTE_REMOVE = process.env.MIO_REMOTE_REMOVE === "1";

// Slug-keyed so two worktrees never share a sandbox root. Under $HOME because only the user
// profile is auto-mounted into the podman VM (needed by the standard tier's bind mount). Read via
// SANDBOX_WORKSPACE_ROOT in src-tauri/src/backends/.
process.env.SANDBOX_WORKSPACE_ROOT = WORKSPACE_ROOT;

// MSB_HOME isolates microsandbox's own state (db, sandboxes, cache, logs) per worktree, mirroring
// SANDBOX_WORKSPACE_ROOT above, and is read by the `msb` binary itself (upstream `resolve_home()`).
// No MSB_PATH export: the app always runs its bundled `msb` and overrides the variable itself
// (docs/features/microsandbox-provider/02_dev.md, "Implemented: bundled `msb` runtime").
process.env.MSB_HOME = MSB_HOME;

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

// The app writes app-data (settings, credentials, models, Mutagen data, logs) under this slug-keyed
// root instead of the developer's real %APPDATA%/com.tincho.sandbox-mio (src-tauri/src/paths.rs).
// Only the test model's own subdirectory is cleaned up in onComplete — re-downloading a ~100 MB
// GGUF per run is not worth the disk it saves.
process.env.MIO_APP_DATA_DIR = APP_DATA_ROOT;
const APP_DATA_MODELS = path.join(APP_DATA_ROOT, "models");

// Redirected rather than cleaned in place: a real captured bundle would flip AgentManager.svelte to
// "Use saved login", breaking agent-setup.spec.ts. Subsystem-specific override
// (src-tauri/src/credentials/mod.rs `store_dir`) takes precedence over MIO_APP_DATA_DIR, so this
// still has to be set explicitly.
const E2E_CREDENTIALS_DIR = path.join(APP_DATA_ROOT, "credentials");
process.env.MIO_CREDENTIALS_DIR = E2E_CREDENTIALS_DIR;

// Persisted app settings (src-tauri/src/settings/mod.rs `settings_path`) are redirected for the
// same reason: each run must start from the Rust defaults, and resetting them in place would
// delete the release build's real settings file.
const E2E_SETTINGS_DIR = path.join(APP_DATA_ROOT, "settings");
process.env.MIO_SETTINGS_DIR = E2E_SETTINGS_DIR;

// Kills target/debug processes left over from a previous run (they can outlive their parent,
// sync/mod.rs `shutdown`) — a stale one locks its .exe and fails the build below (LNK1104).
// Matched against this worktree's own absolute src-tauri\target\debug\ path (via env var, compared
// with StartsWith/OrdinalIgnoreCase — not PowerShell -like, which gives `[`, `]`, `*`, `?` wildcard
// meaning) so a run in one worktree never kills another worktree's stray processes, including a
// worktree nested under this one's .claude/worktrees/.
function killStaleDevProcesses(): void {
  if (process.platform !== "win32") return;
  spawnSync(
    "powershell",
    [
      "-NoProfile",
      "-Command",
      "Get-Process -Name podman,win-sshproxy,gvproxy,sandbox-mio,mutagen -ErrorAction SilentlyContinue | " +
        "Where-Object { $_.Path -and $_.Path.StartsWith($env:MIO_DEBUG_DIR, [StringComparison]::OrdinalIgnoreCase) } | " +
        "Stop-Process -Force -ErrorAction SilentlyContinue",
    ],
    { stdio: "ignore", env: { ...process.env, MIO_DEBUG_DIR: DEBUG_DIR } },
  );
}

// Hard-links (falling back to a byte copy) when the destination is missing or a different size —
// the assets are version-pinned, so a same-size destination is already the right file. Skipping the
// re-copy is what keeps a light run from moving ~180 MB of pinned assets on every iteration of the
// loop this feature exists to serve (docs/features/parallel-worktree-e2e/02_dev.md Phase F).
function stageIfMissing(src: string, dst: string): void {
  if (fs.existsSync(dst) && fs.statSync(dst).size === fs.statSync(src).size) return;
  fs.rmSync(dst, { force: true });
  try {
    fs.linkSync(src, dst);
  } catch {
    fs.copyFileSync(src, dst);
  }
}

function stageTreeIfMissing(src: string, dst: string): void {
  for (const entry of fs.readdirSync(src, { withFileTypes: true })) {
    const from = path.join(src, entry.name);
    const to = path.join(dst, entry.name);
    if (entry.isDirectory()) {
      fs.mkdirSync(to, { recursive: true });
      stageTreeIfMissing(from, to);
    } else {
      stageIfMissing(from, to);
    }
  }
}

let driver: ChildProcess | undefined;
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

// Starting value only. A freshly forked worker inherits the port the launcher allocated (process.env
// as of onPrepare's return — see run-identity.ts IS_LAUNCHER), and the launcher's own first parse
// runs before onPrepare so it falls back to the window base. Either way `beforeSession` overwrites
// `config.port` before a session is created.
const PORT = Number(process.env.MIO_DRIVER_PORT ?? PORT_WINDOW_BASE);

export const config: WebdriverIO.Config = {
  hostname: "127.0.0.1",
  port: PORT,
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
      // msedgedriver's own WebView2 form of what tauri-driver's `tauri:options` used to translate to.
      browserName: "webview2",
      "ms:edgeOptions": {
        binary: path.resolve(__dirname, `../src-tauri/target/debug/sandbox-mio${ext}`),
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
  onPrepare: async (wdioConfig) => {
    // Taken before any deletion below, so a second run in this worktree (or a second real-infra run
    // anywhere on the host) aborts before it can destroy this run's directories.
    if (IS_LAUNCHER) {
      try {
        acquireWorktreeLock();
      } catch (e) {
        console.error(e instanceof Error ? e.message : String(e));
        process.exit(1);
      }
      if (!IS_LIGHT_RUN) {
        try {
          acquireHostLock();
        } catch (e) {
          console.error(e instanceof Error ? e.message : String(e));
          releaseWorktreeLock();
          process.exit(1);
        }
      }
    }

    // Provisional: the port a worker inherits, and what the line below reports. `beforeSession` is
    // the authority — it re-probes and reassigns `config.port` per session, because only that hook
    // can keep a retry's new port and the session's client in step (see spawnDriver).
    const driverPort = await allocateDriverPort();
    process.env.MIO_DRIVER_PORT = String(driverPort);
    wdioConfig.port = driverPort;

    console.log(
      `[e2e] run slug=${RUN_SLUG} port=${driverPort} ` +
        `(window #${PORT_WINDOW_INDEX} base ${PORT_WINDOW_BASE}, MIO_PORT_BASE=${MIO_PORT_BASE}) ` +
        `workspace=${WORKSPACE_ROOT} appData=${APP_DATA_ROOT}`,
    );

    fs.writeFileSync(FAILURES_LOG, "");
    fs.rmSync(WORKSPACE_ROOT, { recursive: true, force: true });
    // Wiped wholesale, not cleaned in place: settings must start from Rust defaults or a prior
    // aborted run pollutes test assertions; credentials must start empty or agent-setup.spec.ts
    // sees "Use saved login" and fails.
    fs.rmSync(E2E_SETTINGS_DIR, { recursive: true, force: true });
    fs.rmSync(E2E_CREDENTIALS_DIR, { recursive: true, force: true });
    // The one field not left at its Rust default: `onboardingCompleted: false` hides the nav chrome
    // (src/routes/+layout.svelte), which every spec navigating by tab depends on. A welcome-flow
    // spec resets the flag itself to reach the first-run path.
    fs.mkdirSync(E2E_SETTINGS_DIR, { recursive: true });
    fs.writeFileSync(
      path.join(E2E_SETTINGS_DIR, "settings.json"),
      JSON.stringify({ onboardingCompleted: true }),
    );
    if (IS_LIGHT_RUN) {
      console.log("[e2e] light suite: skipping podman machine provisioning (MIO_MOCK_BACKEND=1).");
    } else {
      // Init only, not started: podman-startup-recovery.spec.ts stops this machine itself, and every
      // other session's probe follows MIO_PODMAN_MACHINE onto LOCAL_CONNECTION instead.
      try {
        ensureMachineInitialized(DEFAULT_MACHINE);
      } catch (e) {
        console.warn(
          `[e2e] ${DEFAULT_MACHINE} could not be initialized: ${e instanceof Error ? e.message : String(e)}`,
        );
        console.warn("[e2e] podman-startup-recovery.spec.ts will likely fail.");
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
    // would leave no exe (or a stale one) for msedgedriver to launch.
    //
    // --no-default-features drops the `devtools` feature (src-tauri/Cargo.toml): tauri-plugin-devtools
    // claims the process's one global tracing subscriber before `init_logging()` runs, so with it
    // present `init_logging()`'s own `try_init()` silently no-ops and mio.log is never written —
    // confirmed by running the debug exe standalone (docs/features/parallel-worktree-e2e/03_e2e.md,
    // De-risking spike results). Nothing under e2e/test/specs/ exercises devtools.
    const build = spawnSync(
      "pnpm",
      // `--no-default-features` is a cargo flag, not a tauri one: it must come after `--` or the
      // tauri CLI rejects it as an unexpected argument.
      ["tauri", "build", "--debug", "--no-bundle", "--", "--no-default-features"],
      {
        cwd: path.resolve(__dirname, ".."),
        stdio: "inherit",
        shell: true,
      },
    );
    if (build.status !== 0 || !fs.existsSync(appExe)) {
      console.error(`[e2e] tauri build --debug failed (exit ${build.status}); ${appExe} not produced.`);
      console.error("[e2e] Aborting suite — cannot run specs without a freshly built app binary.");
      process.exit(1);
    }
    // Mutagen needs its agent bundle adjacent to the sidecar binary to install agents on new SSH
    // endpoints. Sourced from src-tauri/binaries/, which `build dev --setup-worktree` and
    // `build dev --setup` both stage — not tools/, which carries a second, unused copy
    // (docs/features/parallel-worktree-e2e/04_improvements.md).
    const mutagenAgentsSrc = path.resolve(__dirname, "../src-tauri/binaries/mutagen-agents.tar.gz");
    const mutagenAgentsDst = path.resolve(__dirname, "../src-tauri/target/debug/mutagen-agents.tar.gz");
    if (fs.existsSync(mutagenAgentsSrc)) {
      stageIfMissing(mutagenAgentsSrc, mutagenAgentsDst);
    } else {
      console.warn(
        `[e2e] ${mutagenAgentsSrc} not staged; run \`build dev --setup-worktree\` (or \`build dev --setup\`` +
          " in an already-staged worktree) to fetch it.",
      );
    }
    // --no-bundle skips Tauri `resources`, so stage llama.cpp next to the exe for resource_dir()/llama
    // (src-tauri/src/inference/llama.rs). Skipped with a warning if not staged, so only
    // local-llm.spec.ts fails rather than the whole suite. See docs/features/local-llm/03_e2e.md.
    const llamaSrc = path.resolve(__dirname, "../src-tauri/binaries/llama");
    const llamaDst = path.resolve(__dirname, "../src-tauri/target/debug/llama");
    if (fs.existsSync(path.join(llamaSrc, `llama-server${ext}`))) {
      fs.mkdirSync(llamaDst, { recursive: true });
      stageTreeIfMissing(llamaSrc, llamaDst);
    } else {
      console.warn(
        `[e2e] llama-server not staged at ${llamaSrc}; local-llm.spec.ts will fail at sidecar ` +
          "resolution. Stage the llama.cpp Vulkan release there per docs/features/local-llm/03_e2e.md.",
      );
    }
  },
  beforeSession: async (sessionConfig, _capabilities, specs) => {
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
    // This spec's hooks depend on the probe and the sandbox backend resolving different machines.
    const isRecoverySession = specs.some((s) => s.includes("podman-startup-recovery"));
    await spawnDriver(sessionConfig, specs, isLightSession, isRecoverySession);
  },
  // One merged chrome webview hosts the whole app UI (docs/features/consolidate-app-container/02_dev.md);
  // each Terminal/AgentWeb pane opens its own overlay webview. Pin the session to the app UI
  // handle so it doesn't land on a pane's window instead.
  // A webview still on `about:blank` has not loaded the app yet; pinning it made every later
  // `execute` fail with "Origin header is not a valid URL", so wait for a loaded app page instead.
  // `getUrl` is a WebDriver command, so it also answers on a page no script can run in yet.
  before: async () => {
    await browser.waitUntil(
      async () => {
        for (const handle of await browser.getWindowHandles()) {
          await browser.switchToWindow(handle);
          const url = new URL(await browser.getUrl());
          if (url.protocol !== "about:" && !url.pathname.startsWith("/workspace")) return true;
        }
        return false;
      },
      { timeout: 30000, interval: 250, timeoutMsg: "[e2e] no loaded app webview found among the window handles" },
    );
  },
  afterTest(_test, _context, result) {
    logFailure(_test.fullTitle || `${_test.parent} > ${_test.title}`, result);
  },
  // Suite-level before/after hooks (e.g. podman-startup-recovery.spec.ts's `before()`) fail outside
  // any `it()`, so afterTest above never sees them — without this, a hook failure left
  // FAILURES_LOG empty and the reporter's only trace was mocha's own "N passing" summary.
  afterHook(_test, _context, result, hookName) {
    logFailure(`${_test.fullTitle || _test.parent} > ${hookName}`, result);
  },
  afterSession: () => {
    closeDriver();
  },
  onComplete: () => {
    fs.rmSync(WORKSPACE_ROOT, { recursive: true, force: true });

    // Remove the downloaded test GGUF so it does not persist or grow the disk across runs. Only
    // the test model's own subdirectory is deleted.
    fs.rmSync(path.join(APP_DATA_MODELS, TEST_MODEL.id), { recursive: true, force: true });

    // Tear down real-infra-only state. A light run writes neither the SSH alias nor the remote
    // machine, so both used to be pure collateral damage against a concurrent standard-tier run in
    // another worktree; the host lock above now also means no *other* real-infra run can be mid-way
    // through writing them when this runs.
    if (!IS_LIGHT_RUN) {
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

      if (MIO_REMOTE_REMOVE) removeMachine(REMOTE_CONNECTION);
      else if (!MIO_REMOTE_KEEP) stopMachine(REMOTE_CONNECTION);
    }

    if (IS_LAUNCHER) {
      // Diagnostic only, never fatal: the Mutagen daemon is shared per data directory and outlives
      // the app, so leftovers here are expected across concurrent worktrees — but a climbing count
      // is the early signal that daemons are leaking (each carries the environment of whichever CLI
      // call auto-started it, the hazard behind the mangled-`mio-sshd` failure). Logged, not swept.
      const daemons = listMutagenDaemons();
      console.log(`[e2e] ${daemons.length} mutagen daemon(s) still running at end of run:`);
      for (const { pid, started } of daemons) {
        console.log(`[e2e]   pid=${pid} started=${started}`);
      }

      releaseWorktreeLock();
      if (!IS_LIGHT_RUN) releaseHostLock();
    }
  },
};

const stripAnsi = (s: string) => s.replace(/\x1B\[[0-9;]*[A-Za-z]/g, "");

// Shared by afterTest and afterHook so a suite hook failure is logged identically to a test failure.
function logFailure(title: string, result: { passed: boolean; error?: Error }): void {
  if (result.passed || !result.error) return;
  const err = result.error as Error & { stack?: string };
  const message = stripAnsi(err.message ?? String(err));
  const stack = stripAnsi(err.stack ?? "");
  fs.appendFileSync(FAILURES_LOG, [`FAILED: ${title}`, message, stack, ""].join("\n"));
}

// The driver dying takes the whole worker down via process.exit before mocha can report
// anything — write straight to FAILURES_LOG so the in-flight spec isn't a silent gap.
function logCrash(message: string, specs: string[]): void {
  fs.appendFileSync(FAILURES_LOG, [`FAILED: ${specs.join(", ") || "(unknown spec)"}`, message, ""].join("\n"));
}

// How long a driver gets to bind its port before the attempt counts as failed. Generous: the
// readiness poll ends the wait as soon as the port is taken, so this only bounds the pathological
// case.
const DRIVER_READY_TIMEOUT_MS = 20000;
const DRIVER_READY_POLL_MS = 100;

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

// Connects rather than probing with isPortFree(): that one answers by *binding*, which during
// startup races the driver for the very port it is trying to claim.
function isPortListening(port: number): Promise<boolean> {
  return new Promise((resolve) => {
    const socket = net.connect({ port, host: "127.0.0.1" });
    const done = (listening: boolean) => {
      socket.destroy();
      resolve(listening);
    };
    socket.once("connect", () => done(true));
    socket.once("error", () => done(false));
    socket.setTimeout(1000, () => done(false));
  });
}

// Resolves true once the driver has taken `port`, false as soon as it is clear it never will (the
// child exited, spawn failed, or the timeout elapsed). An explicit readiness check rather than the
// old "did it exit within 5 s" heuristic, so the retry decision is "did it come up" instead of
// "how fast did it die".
async function waitForDriverReady(
  child: ChildProcess,
  port: number,
  spawnFailure: () => string | undefined,
): Promise<boolean> {
  const deadline = Date.now() + DRIVER_READY_TIMEOUT_MS;
  while (Date.now() < deadline) {
    if (spawnFailure() !== undefined) return false;
    if (child.exitCode !== null || child.signalCode !== null) return false;
    if (await isPortListening(port)) return true;
    await sleep(DRIVER_READY_POLL_MS);
  }
  return false;
}

// Allocates the session's port, spawns msedgedriver on it, and assigns it to `config.port`.
//
// msedgedriver is spawned directly rather than through tauri-driver: tauri-driver puts itself and
// every descendant in a Windows job object that denies breakaway, and `msb create` must break away
// to start its VM supervisor (docs/features/microsandbox-provider/03_e2e.md, Step 0).
//
// Allocation lives here, not in onPrepare, because this hook owns the config the WebdriverIO
// session is built from: a retry that rebinds the driver to a different port must move the client
// with it, or the session dials the dead port and a clear "driver exited" failure degrades into a
// connection timeout. A port window is a strong default, not a guarantee two worktrees never
// collide (run-identity.ts), so a driver that never comes up is retried once on a freshly probed
// port — both before this promise resolves, hence before the session exists.
async function spawnDriver(
  // Structural, not `WebdriverIO.Config`: beforeSession is handed an `Omit<Testrunner,
  // "capabilities">`, and `port` is the only field this touches.
  config: { port?: number },
  specs: string[],
  isLightSession: boolean,
  isRecoverySession: boolean,
): Promise<void> {
  const ATTEMPTS = 2;
  for (let attempt = 1; attempt <= ATTEMPTS; attempt++) {
    const driverPort = await allocateDriverPort();
    // Published for the app and for light/run-identity.spec.ts's port assertion; config.port is
    // what the session itself connects to.
    process.env.MIO_DRIVER_PORT = String(driverPort);
    config.port = driverPort;

    let spawnFailure: string | undefined;
    const child = spawn(
      path.resolve(__dirname, "..", "tools", "msedgedriver.exe"),
      [`--port=${driverPort}`],
      {
        // stdout is the driver's own chatter, kept out of the reporter's output.
        stdio: ["ignore", "ignore", process.stderr],
        env: {
          ...process.env,
          // Tauri only exposes the webview to WebDriver when this is set.
          TAURI_WEBVIEW_AUTOMATION: "true",
          PODMAN_CONNECTION: LOCAL_CONNECTION,
          // Left unset for the recovery spec so its probe falls back to DEFAULT_MACHINE_NAME.
          ...(isLightSession || isRecoverySession ? {} : { MIO_PODMAN_MACHINE: LOCAL_CONNECTION }),
          ...(isLightSession ? { MIO_MOCK_BACKEND: "1" } : {}),
        },
      },
    );
    driver = child;
    // Captured rather than fatal while the readiness poll is still running: a failure here is one
    // failed attempt, and the retry below decides whether it ends the run.
    child.on("error", (error) => {
      spawnFailure = `msedgedriver error: ${error.message ?? error}`;
    });

    if (await waitForDriverReady(child, driverPort, () => spawnFailure)) {
      // From here on, any exit is unexpected. process.exit takes the worker down before mocha can
      // report anything, so the crash is written to FAILURES_LOG synchronously first — otherwise
      // the in-flight spec leaves no trace at all.
      if (spawnFailure !== undefined) {
        logCrash(spawnFailure, specs);
        process.exit(1);
      }
      child.on("error", (error) => {
        logCrash(`msedgedriver error: ${error.message ?? error}`, specs);
        process.exit(1);
      });
      child.on("exit", (code) => {
        if (exit) return;
        logCrash(`msedgedriver exited unexpectedly with code: ${code}`, specs);
        process.exit(1);
      });
      return;
    }

    killDriverTree(child);
    const why = spawnFailure ?? `it did not bind the port (exit code ${child.exitCode})`;
    if (attempt === ATTEMPTS) {
      logCrash(`msedgedriver failed to start on port ${driverPort}: ${why}`, specs);
      process.exit(1);
    }
    console.warn(
      `[e2e] msedgedriver did not come up on port ${driverPort} (${why}); ` +
        "retrying once against a freshly probed port.",
    );
  }
}

// Kills the driver and the app it launched: msedgedriver dying alone leaves the app running.
function killDriverTree(child: ChildProcess): void {
  if (child.pid === undefined || child.exitCode !== null) return;
  spawnSync("taskkill", ["/T", "/F", "/PID", String(child.pid)], { stdio: "ignore" });
}

function closeDriver(): void {
  exit = true;
  if (driver) killDriverTree(driver);
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
  closeDriver();
  // The config module (and this registration) runs in every worker too, not only the launcher; an
  // ungated release would drop the lock as soon as the first spec file's worker exited
  // (docs/features/parallel-worktree-e2e/02_dev.md, Resolved Decision 8).
  if (IS_LAUNCHER) {
    releaseWorktreeLock();
    if (!IS_LIGHT_RUN) releaseHostLock();
  }
});
