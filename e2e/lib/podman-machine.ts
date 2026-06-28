import { spawnSync } from "child_process";
import { readFileSync } from "fs";
import path from "path";
import { fileURLToPath } from "url";

// Rootless Podman API socket inside a machine (UID 1000). Both the local test machine and the
// remote stand-in run rootless, so the socket path is the same on each.
export const PODMAN_SOCKET = "/run/user/1000/podman/podman.sock";

// Base image for all sandboxes — mise + zellij baked in. Name and version are read directly from
// src-tauri/src/backends/mod.rs so this file never needs a manual update when the version bumps.
function readBaseImage(): string {
  const src = readFileSync(
    fileURLToPath(new URL("../../src-tauri/src/backends/mod.rs", import.meta.url)),
    "utf8"
  );
  const name = src.match(/const BASE_IMAGE:\s*&str\s*=\s*"([^"]+)"/)?.[1];
  const version = src.match(/const BASE_IMAGE_VERSION:\s*u32\s*=\s*(\d+)/)?.[1];
  if (!name || !version) throw new Error("Could not parse BASE_IMAGE / BASE_IMAGE_VERSION from backends/mod.rs");
  return `${name}:${version}`;
}
export const MISE_BASE_IMAGE = readBaseImage();
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

// Poll `podman --connection <name> info` until it succeeds — the same readiness check the app
// makes in verify_host. Returns true on success, false on timeout so the caller can re-launch
// the service and retry.
export function waitForConnection(connectionName: string, timeoutMs = 20000): boolean {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    if (spawnSync("podman", ["--connection", connectionName, "info"], { shell: true }).status === 0) {
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
  spawnSync(
    "podman",
    [
      "machine", "ssh", machineName,
      `test -S ${PODMAN_SOCKET} || { mkdir -p $(dirname ${PODMAN_SOCKET}) && setsid env XDG_RUNTIME_DIR=/run/user/1000 podman system service --time=0 </dev/null >/tmp/podman-svc.log 2>&1 & }`,
    ],
    { stdio: "inherit", shell: false, timeout: 7000, killSignal: "SIGTERM" }
  );
}

// Bring a podman machine up and ready: init if missing, start if stopped, activate the rootless
// API socket (launch-and-poll, retried because the socket sometimes fails to bind first try on
// the WSL image), then pre-pull the requested images. Idempotent — safe to call every run.
export function provisionMachine(
  machineName: string,
  { pullImages = ["docker.io/library/alpine:latest"] }: { pullImages?: string[] } = {}
): void {
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
    spawnSync("podman", ["machine", "init", machineName], { stdio: "inherit", shell: true });
  }

  if (!machine?.Running) {
    console.log(`Starting podman machine "${machineName}"...`);
    const startResult = spawnSync("podman", ["machine", "start", machineName], { stdio: "inherit", shell: true });
    if (startResult.status !== 0) {
      // HCS_E_CONNECTION_TIMEOUT and similar WSL2 bootstrap failures leave the WSL kernel service
      // in a stuck state. A wsl --shutdown resets all instances cleanly; the machine can then be
      // started on the next attempt. Only attempted once — if shutdown+start also fails, the
      // machine is genuinely broken and we should not retry further.
      console.log(`  machine start failed (exit ${startResult.status}), attempting wsl --shutdown recovery...`);
      spawnSync("wsl", ["--shutdown"], { stdio: "inherit", shell: true });
      sleepSync(2000);
      spawnSync("podman", ["machine", "start", machineName], { stdio: "inherit", shell: true });
    }
  }

  console.log(`Activating rootless Podman socket on "${machineName}"...`);
  const SOCKET_ATTEMPTS = 4;
  let ready = false;
  for (let attempt = 1; attempt <= SOCKET_ATTEMPTS && !ready; attempt++) {
    startPodmanSocket(machineName);
    ready = waitForConnection(machineName);
    if (!ready) {
      console.log(`  "${machineName}" not ready (attempt ${attempt}/${SOCKET_ATTEMPTS}), retrying...`);
    }
  }
  if (!ready) {
    throw new Error(`Podman connection "${machineName}" did not become ready after ${SOCKET_ATTEMPTS} attempts`);
  }

  for (const image of pullImages) {
    console.log(`Ensuring ${image} is available on "${machineName}"...`);
    spawnSync("podman", ["machine", "ssh", machineName, `podman pull ${image}`], {
      stdio: "inherit",
      shell: true,
    });
  }
}

// Idempotent: build MISE_BASE_IMAGE on localName if absent, then stage onto remoteNames via
// save|load. `podman image scp` is not used — it fails between machine connections on Windows.
export function ensureMiseBaseImage(localName: string, remoteNames: string[] = []): void {
  const has = (name: string) =>
    spawnSync("podman", ["--connection", name, "image", "exists", MISE_BASE_IMAGE], {
      shell: false,
    }).status === 0;

  if (!has(localName)) {
    console.log(`Building ${MISE_BASE_IMAGE} on "${localName}" (one-time, ~324 MB)...`);
    const build = spawnSync(
      "podman",
      ["--connection", localName, "build", "-t", MISE_BASE_IMAGE, "-f", "Containerfile", "."],
      { cwd: RECIPE_DIR, stdio: "inherit", shell: false }
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
  spawnSync("podman", ["machine", "stop", machineName], { stdio: "inherit", shell: true });
}

// Delete a machine and its disk image (contrast stopMachine, which keeps the image).
export function removeMachine(machineName: string): void {
  spawnSync("podman", ["machine", "rm", "-f", machineName], { stdio: "inherit", shell: true });
}

// Parse the SSH transport details for a system connection from `podman system connection list`.
// The port is assigned dynamically at machine boot, so this must be read fresh each run rather
// than cached. Returns { user, host, port, identity }.
export function connectionInfo(connectionName: string): ConnectionInfo {
  const out = spawnSync("podman", ["system", "connection", "list", "--format", "json"], {
    encoding: "utf8",
    shell: true,
  });
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
