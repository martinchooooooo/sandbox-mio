import {
  spawnSync,
  type SpawnSyncOptions,
  type SpawnSyncOptionsWithStringEncoding,
  type SpawnSyncReturns,
} from "child_process";
import { readFileSync } from "fs";
import path from "path";
import { fileURLToPath } from "url";

// Rootless (UID 1000); both the local test machine and the remote stand-in run rootless.
export const PODMAN_SOCKET = "/run/user/1000/podman/podman.sock";

// Read out of backends/mod.rs rather than duplicated, so a BASE_IMAGE_VERSION bump needs no edit here.
const BACKENDS_SRC = readFileSync(
  fileURLToPath(new URL("../../src-tauri/src/backends/mod.rs", import.meta.url)),
  "utf8"
);
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

/** SSH transport details for a system connection, parsed from `podman system connection list`. */
export interface ConnectionInfo {
  user: string;
  host: string;
  port: string;
  identity: string;
}

interface MachineListEntry {
  Name: string;
  Running?: boolean;
}

interface SystemConnectionEntry {
  Name: string;
  URI: string;
  Identity: string;
}

/** Synchronous: provisioning runs before WebdriverIO spins up its event loop. */
export function sleepSync(ms: number): void {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}

/** Same readiness check the app makes in verify_host. False on timeout, so callers can retry. */
function podmanHost(args: string[], opts: SpawnSyncOptionsWithStringEncoding): SpawnSyncReturns<string>;
function podmanHost(args: string[], opts?: SpawnSyncOptions): SpawnSyncReturns<Buffer>;
function podmanHost(args: string[], opts: SpawnSyncOptions = {}) {
  return spawnSync("podman", args, { shell: false, ...opts });
}
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

/**
 * Starts the rootless Podman API service inside the machine; idempotent. Launched directly rather
 * than via `systemctl --user`: this minimal WSL image's per-user systemd manager never starts.
 */
export function startPodmanSocket(machineName: string): void {
  podmanHost(
    [
      "machine", "ssh", machineName,
      // setsid is load-bearing: a new session survives the SIGHUP that SSH sends when the channel
      // closes, which plain `& disown` does not.
      `test -S ${PODMAN_SOCKET} || { mkdir -p $(dirname ${PODMAN_SOCKET}) && setsid env XDG_RUNTIME_DIR=/run/user/1000 podman system service --time=0 </dev/null >/tmp/podman-svc.log 2>&1 & }`,
    ],
    // shell:false keeps cmd.exe from eating the shell metacharacters before the remote bash sees
    // them. The pty blocks until the timeout; SIGTERM closes it cleanly where SIGKILL would take
    // the detached service down with it.
    { stdio: "inherit", shell: false, timeout: 7000, killSignal: "SIGTERM" }
  );
}

const SOCKET_ATTEMPTS = 4;

/**
 * Launch-and-poll the rootless API socket, retried because it sometimes fails to bind on the first
 * try on the WSL image. Cheap when already live — startPodmanSocket's `test -S` short-circuits.
 */
export function activatePodmanSocket(machineName: string, attempts = SOCKET_ATTEMPTS): boolean {
  console.log(`Activating rootless Podman socket on "${machineName}"...`);
  for (let attempt = 1; attempt <= attempts; attempt++) {
    startPodmanSocket(machineName);
    if (waitForConnection(machineName)) return true;
    console.log(`  "${machineName}" not ready (attempt ${attempt}/${attempts}), retrying...`);
  }
  return false;
}

function tryStart(machineName: string) {
  return podmanHost(["machine", "start", machineName], {
    stdio: "inherit",
    shell: false,
    timeout: 60000,
  });
}

function describeFailure(result: ReturnType<typeof tryStart>): string {
  return result.signal ? `timed out after 60000ms` : `exit ${result.status}`;
}

interface RecoveryStep {
  /** Announcement printed before the step runs, given the failure that triggered it. */
  label: (failure: string) => string;
  run: (machineName: string) => void;
}

const START_RECOVERY_STEPS: RecoveryStep[] = [
  {
    label: (failure) => `  machine start failed (${failure}), attempting wsl --shutdown recovery...`,
    run: () => {
      spawnSync("wsl", ["--shutdown"], { stdio: "inherit", shell: true });
      sleepSync(2000);
    },
  },
  {
    label: (failure) => `  machine start still failing (${failure}), resetting the machine record...`,
    run: (machineName) => {
      podmanHost(["machine", "rm", "-f", machineName], { stdio: "inherit" });
      spawnSync("wsl", ["--unregister", machineName], { stdio: "inherit", shell: true });
      console.log(`Recreating podman machine "${machineName}"...`);
      podmanHost(["machine", "init", machineName], { stdio: "inherit" });
    },
  },
];

/** Idempotent — safe to call every run. */
export function provisionMachine(machineName: string): void {
  const listResult = spawnSync("podman", ["machine", "list", "--format", "json"], {
    encoding: "utf8",
    shell: true,
  });

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

/** Staged via save|load rather than `podman image scp`, which fails between machines on Windows. */
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

/** Leaves the WSL image on disk so the next run skips init; contrast removeMachine. */
export function stopMachine(machineName: string): void {
  podmanHost(["machine", "stop", machineName], { stdio: "inherit" });
}

/** Deletes the machine and its disk image; contrast stopMachine. */
export function removeMachine(machineName: string): void {
  podmanHost(["machine", "rm", "-f", machineName], { stdio: "inherit" });
}

/** Never cache the result: the port is assigned dynamically at machine boot. */
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
