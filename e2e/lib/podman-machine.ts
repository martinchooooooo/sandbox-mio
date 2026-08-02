import {
  spawnSync,
  type SpawnSyncOptions,
  type SpawnSyncOptionsWithStringEncoding,
  type SpawnSyncReturns,
} from "child_process";
import { readFileSync } from "fs";
import path from "path";
import { fileURLToPath } from "url";

// Rootless Podman API socket inside a machine (UID 1000). Both the local test machine and the
// remote stand-in run rootless, so the socket path is the same on each.
export const PODMAN_SOCKET = "/run/user/1000/podman/podman.sock";

// Single source for every constant this file mirrors out of the Rust backend; a renamed or retyped
// declaration there breaks the regexes below rather than silently desyncing.
const BACKENDS_SRC = readFileSync(
  fileURLToPath(new URL("../../src-tauri/src/backends/mod.rs", import.meta.url)),
  "utf8"
);

// Base image for all sandboxes — mise + zellij baked in. Parsed from `BASE_IMAGE`/
// `BASE_IMAGE_VERSION` in src-tauri/src/backends/mod.rs, so a version bump needs no update here.
function readBaseImage(): string {
  const name = BACKENDS_SRC.match(/const BASE_IMAGE:\s*&str\s*=\s*"([^"]+)"/)?.[1];
  const version = BACKENDS_SRC.match(/const BASE_IMAGE_VERSION:\s*u32\s*=\s*(\d+)/)?.[1];
  if (!name || !version) throw new Error("Could not parse BASE_IMAGE / BASE_IMAGE_VERSION from backends/mod.rs");
  return `${name}:${version}`;
}
export const MISE_BASE_IMAGE = readBaseImage();

// The machine the app's own startup probe hardcodes — not the connection the suite points the app
// at (LOCAL_CONNECTION via PODMAN_CONNECTION), so both must be provisioned. Parsed from
// `DEFAULT_MACHINE_NAME` in src-tauri/src/backends/mod.rs so a change there cannot desync silently.
function readDefaultMachine(): string {
  const name = BACKENDS_SRC.match(/const DEFAULT_MACHINE_NAME:\s*&str\s*=\s*"([^"]+)"/)?.[1];
  if (!name) throw new Error("Could not parse DEFAULT_MACHINE_NAME from backends/mod.rs");
  return name;
}
export const DEFAULT_MACHINE = readDefaultMachine();
const RECIPE_DIR = path.resolve(
  fileURLToPath(new URL(".", import.meta.url)),
  "../../src-tauri/resources"
);

// Transport details for a system connection, parsed from `podman system connection list`.
export interface ConnectionInfo {
  user: string;
  host: string;
  port: string;
  identity: string;
}

// Shape of the entries returned by `podman machine list --format json` (only the fields used).
interface MachineListEntry {
  Name: string;
  Running?: boolean;
}

// Shape of the entries returned by `podman system connection list --format json`.
interface SystemConnectionEntry {
  Name: string;
  URI: string;
  Identity: string;
}

// Synchronous sleep — provisioning runs before WebdriverIO spins up its event loop and the
// node:test setup runs serially, so the steps below are all synchronous (spawnSync).
export function sleepSync(ms: number): void {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}

/**
 * Runs the host `podman` CLI, defaulting to `shell: false` so argv reaches podman verbatim.
 * The two calls that depend on that restate it explicitly rather than inherit it.
 */
function podmanHost(args: string[], opts: SpawnSyncOptionsWithStringEncoding): SpawnSyncReturns<string>;
function podmanHost(args: string[], opts?: SpawnSyncOptions): SpawnSyncReturns<Buffer>;
function podmanHost(args: string[], opts: SpawnSyncOptions = {}) {
  return spawnSync("podman", args, { shell: false, ...opts });
}

// Poll `podman --connection <name> info` until it succeeds — the same readiness check the app
// makes in verify_host. Returns true on success, false on timeout so the caller can re-launch
// the service and retry.
export function waitForConnection(connectionName: string, timeoutMs = 20000): boolean {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    if (podmanHost(["--connection", connectionName, "info"]).status === 0) {
      return true;
    }
    if (Date.now() >= deadline) {
      return false;
    }
    sleepSync(500);
  }
}

// Launch the rootless Podman API service inside the machine. Idempotent: skips if the socket
// already exists.
//
// This podman machine is a minimal WSL image whose per-user systemd manager (user@1000.service)
// fails to start, so the tutorial's `systemctl --user enable --now podman.socket` has no
// session bus to reach. The rootless service is launched directly instead — the same socket,
// without systemd. `setsid` is the crucial part: it runs the service in a new session so it
// survives the SIGHUP that SSH sends when the channel closes (plain `& disown` is killed by
// that SIGHUP here, leaving no socket). `podman machine ssh` allocates a pty and so blocks
// until the timeout even after the service is backgrounded; the timeout then SIGTERMs the SSH
// client, which closes the connection cleanly and leaves the detached service running. SIGKILL
// must NOT be used here — it tears the pty down abruptly and takes the service with it.
//
// `shell` is intentionally false: with `shell: true` on Windows the command is routed through
// cmd.exe, which treats `||`, `&`, `>`, `(` and `$(...)` as its own metacharacters and shreds
// the script before it reaches SSH. Spawning `podman` directly passes the command as one argv
// element straight to the remote shell, which is the bash that must interpret it.
export function startPodmanSocket(machineName: string): void {
  podmanHost(
    [
      "machine", "ssh", machineName,
      `test -S ${PODMAN_SOCKET} || { mkdir -p $(dirname ${PODMAN_SOCKET}) && setsid env XDG_RUNTIME_DIR=/run/user/1000 podman system service --time=0 </dev/null >/tmp/podman-svc.log 2>&1 & }`,
    ],
    { stdio: "inherit", shell: false, timeout: 7000, killSignal: "SIGTERM" }
  );
}

const SOCKET_ATTEMPTS = 4;

// Launch-and-poll the rootless API socket, retried because it sometimes fails to bind on the first
// try on the WSL image. Returns whether the connection answered rather than throwing, so callers
// choose: provisioning treats a dead socket as fatal, cleanup hooks only warn. Cheap when the
// socket is already live — startPodmanSocket's `test -S` short-circuits and the first poll passes.
export function activatePodmanSocket(machineName: string, attempts = SOCKET_ATTEMPTS): boolean {
  console.log(`Activating rootless Podman socket on "${machineName}"...`);
  for (let attempt = 1; attempt <= attempts; attempt++) {
    startPodmanSocket(machineName);
    if (waitForConnection(machineName)) return true;
    console.log(`  "${machineName}" not ready (attempt ${attempt}/${attempts}), retrying...`);
  }
  return false;
}

// Bounds every `machine start` attempt: without a timeout, spawnSync blocks forever on a wedged
// WSL2 boot (the failure this exists to recover from), hanging onPrepare — and the whole suite —
// before a single test runs, with no error to explain why. A healthy start finishes well under it.
const MACHINE_START_TIMEOUT_MS = 60000;

// `shell` is intentionally false: under cmd.exe the timeout kills only cmd, leaving podman to
// keep booting the distro that the last START_RECOVERY_STEPS entry then rm/unregisters underneath
// it — manufacturing the desync that step exists to repair. Spawned directly, the timeout
// kills podman itself (podman.exe still resolves via PATHEXT, as the shell-free calls elsewhere do).
function tryStart(machineName: string) {
  return podmanHost(["machine", "start", machineName], {
    stdio: "inherit",
    shell: false,
    timeout: MACHINE_START_TIMEOUT_MS,
  });
}

// A timeout kill leaves status null and signal set, so failure is always `status !== 0`.
function describeFailure(result: ReturnType<typeof tryStart>): string {
  return result.signal ? `timed out after ${MACHINE_START_TIMEOUT_MS}ms` : `exit ${result.status}`;
}

interface RecoveryStep {
  /** Announcement printed before the step runs, given the failure that triggered it. */
  label: (failure: string) => string;
  run: (machineName: string) => void;
}

// Escalating recovery for a failed `machine start`, tried in order until a start succeeds. Order is
// load-bearing: each step is more destructive than the one before, so never reorder or skip ahead.
const START_RECOVERY_STEPS: RecoveryStep[] = [
  {
    // HCS_E_CONNECTION_TIMEOUT and similar WSL2 bootstrap failures leave the WSL kernel service
    // stuck; wsl --shutdown resets all instances cleanly and the next start gets a fresh window.
    label: (failure) => `  machine start failed (${failure}), attempting wsl --shutdown recovery...`,
    run: () => {
      spawnSync("wsl", ["--shutdown"], { stdio: "inherit", shell: true });
      sleepSync(2000);
    },
  },
  {
    // Surviving wsl --shutdown means the reset was cosmetic — most often WSL_E_DISTRO_NOT_FOUND,
    // where podman's machine record and its backing WSL distro have gone out of sync. Only a
    // teardown and reinit fixes that, mirroring reset_stuck_machine in
    // src-tauri/src/machine/mod.rs. rm/unregister are best-effort: each errors when its target
    // is already gone.
    label: (failure) => `  machine start still failing (${failure}), resetting the machine record...`,
    run: (machineName) => {
      podmanHost(["machine", "rm", "-f", machineName], { stdio: "inherit" });
      spawnSync("wsl", ["--unregister", machineName], { stdio: "inherit", shell: true });
      console.log(`Recreating podman machine "${machineName}"...`);
      podmanHost(["machine", "init", machineName], { stdio: "inherit" });
    },
  },
];

// Bring a podman machine up and ready: init if missing, start if stopped, activate the rootless
// API socket. Idempotent — safe to call every run.
export function provisionMachine(machineName: string): void {
  const listResult = podmanHost(["machine", "list", "--format", "json"], { encoding: "utf8" });

  let machines: MachineListEntry[] = [];
  try {
    machines = JSON.parse(listResult.stdout || "[]");
  } catch {
    machines = [];
  }

  const machine = machines.find((m) => m.Name === machineName);

  if (!machine) {
    console.log(`Creating podman machine "${machineName}"...`);
    podmanHost(["machine", "init", machineName], { stdio: "inherit" });
  }

  if (!machine?.Running) {
    console.log(`Starting podman machine "${machineName}"...`);
    let startResult = tryStart(machineName);
    for (const step of START_RECOVERY_STEPS) {
      if (startResult.status === 0) break;
      console.log(step.label(describeFailure(startResult)));
      step.run(machineName);
      startResult = tryStart(machineName);
    }
    if (startResult.status !== 0) {
      throw new Error(
        `podman machine start "${machineName}" failed even after a full reset (${describeFailure(startResult)})`
      );
    }
  }

  if (!activatePodmanSocket(machineName)) {
    throw new Error(`Podman connection "${machineName}" did not become ready after ${SOCKET_ATTEMPTS} attempts`);
  }
}

// Idempotent: build MISE_BASE_IMAGE on localName if absent, then stage onto remoteNames via
// save|load. `podman image scp` is not used — it fails between machine connections on Windows.
export function ensureMiseBaseImage(localName: string, remoteNames: string[] = []): void {
  const has = (name: string) =>
    podmanHost(["--connection", name, "image", "exists", MISE_BASE_IMAGE]).status === 0;

  if (!has(localName)) {
    console.log(`Building ${MISE_BASE_IMAGE} on "${localName}" (one-time, ~324 MB)...`);
    const build = podmanHost(
      ["--connection", localName, "build", "-t", MISE_BASE_IMAGE, "-f", "Containerfile", "."],
      { cwd: RECIPE_DIR, stdio: "inherit" }
    );
    if (build.status !== 0) {
      throw new Error(`Failed to build ${MISE_BASE_IMAGE} on "${localName}"`);
    }
  }

  for (const remote of remoteNames) {
    if (has(remote)) continue;
    console.log(`Staging ${MISE_BASE_IMAGE} onto "${remote}" via save | load...`);
    const staged = spawnSync(
      `podman --connection ${localName} save ${MISE_BASE_IMAGE} | podman --connection ${remote} load`,
      { stdio: "inherit", shell: true }
    );
    if (staged.status !== 0) {
      throw new Error(`Failed to stage ${MISE_BASE_IMAGE} onto "${remote}"`);
    }
  }
}

// Stop a machine without deleting it — leaves the WSL image on disk so the next run skips init.
export function stopMachine(machineName: string): void {
  podmanHost(["machine", "stop", machineName], { stdio: "inherit" });
}

// Delete a machine and its disk image (contrast stopMachine, which keeps the image).
export function removeMachine(machineName: string): void {
  podmanHost(["machine", "rm", "-f", machineName], { stdio: "inherit" });
}

// Parse the SSH transport details for a system connection from `podman system connection list`.
// The port is assigned dynamically at machine boot, so this must be read fresh each run rather
// than cached. Returns { user, host, port, identity }.
export function connectionInfo(connectionName: string): ConnectionInfo {
  const out = podmanHost(["system", "connection", "list", "--format", "json"], { encoding: "utf8" });
  if (out.status !== 0) {
    throw new Error(`podman system connection list failed: ${out.stderr}`);
  }
  const entries: SystemConnectionEntry[] = JSON.parse(out.stdout || "[]");
  const entry = entries.find((e) => e.Name === connectionName);
  if (!entry) {
    throw new Error(`no system connection named "${connectionName}"`);
  }
  const url = new URL(entry.URI);
  return {
    user: url.username,
    host: url.hostname,
    port: url.port,
    identity: entry.Identity,
  };
}
