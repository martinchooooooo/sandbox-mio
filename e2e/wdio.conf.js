import fs from "fs";
import os from "os";
import path from "path";
import { spawn, spawnSync } from "child_process";
import { fileURLToPath } from "url";

const __dirname = fileURLToPath(new URL(".", import.meta.url));

const TEST_MACHINE = "mio-tests";

// Isolated workspace root for the app's per-sandbox directories. Set so specs know where the
// app writes them and the developer's real Documents folder is left untouched. Kept under
// $HOME because the podman machine only auto-mounts the user profile into its VM, which the
// bind mount needs. Passed to the app process via SANDBOX_WORKSPACE_ROOT (read in
// src-tauri/src/backends/) and read back by the workspace spec.
const WORKSPACE_ROOT = path.join(os.homedir(), ".sandbox-mio-e2e");
process.env.SANDBOX_WORKSPACE_ROOT = WORKSPACE_ROOT;

let tauriDriver;
let exit = false;

const ext = process.platform === "win32" ? ".exe" : "";

// Rootless Podman API socket inside the machine (UID 1000).
const PODMAN_SOCKET = "/run/user/1000/podman/podman.sock";

// Synchronous sleep — `onPrepare` runs before WebdriverIO spins up its event loop, so the
// setup steps below are all synchronous (spawnSync).
function sleepSync(ms) {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}

// Poll `podman --connection <name> info` until it succeeds — the tutorial's `podman --remote
// info` verification step, and the same check the app makes in verify_host. Returns true on
// success, false on timeout so the caller can re-launch the service and retry.
function waitForConnection(connectionName, timeoutMs = 20000) {
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
// already exists, so it is safe to call once per retry attempt.
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
function startPodmanSocket(machineName) {
  spawnSync(
    "podman",
    [
      "machine", "ssh", machineName,
      `test -S ${PODMAN_SOCKET} || { mkdir -p $(dirname ${PODMAN_SOCKET}) && setsid env XDG_RUNTIME_DIR=/run/user/1000 podman system service --time=0 </dev/null >/tmp/podman-svc.log 2>&1 & }`,
    ],
    { stdio: "inherit", shell: false, timeout: 7000, killSignal: "SIGTERM" }
  );
}

function ensurePodmanMachine(machineName) {
  const listResult = spawnSync("podman", ["machine", "list", "--format", "json"], {
    encoding: "utf8",
    shell: true,
  });

  let machines = [];
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
    spawnSync("podman", ["machine", "start", machineName], { stdio: "inherit", shell: true });
  }

  // Make the rootless Podman API socket available. Mirrors the tutorial's server-side setup
  // (https://github.com/podman-container-tools/podman/blob/main/docs/tutorials/mac_win_client.md#creating-the-first-connection-to-an-external-linux-server),
  // which on a normal host is `systemctl --user enable --now podman.socket` plus
  // `loginctl enable-linger`. The WSL machine has no working per-user systemd to supervise
  // the service, so it sometimes fails to bind the socket on the first try; launch-and-poll
  // is retried before giving up. Re-launching is a no-op once the socket exists.
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

  console.log(`Ensuring alpine:latest is available on "${machineName}"...`);
  spawnSync("podman", ["machine", "ssh", machineName, "podman pull docker.io/library/alpine:latest"], {
    stdio: "inherit",
    shell: true,
  });
}

export const config = {
  hostname: "127.0.0.1",
  port: 4444,
  specs: ["./test/specs/**/*.js"],
  maxInstances: 1,
  capabilities: [
    {
      maxInstances: 1,
      "tauri:options": {
        application: path.resolve(__dirname, `../src-tauri/target/debug/sandbox-mio${ext}`),
      },
    },
  ],
  reporters: ["spec"],
  framework: "mocha",
  mochaOpts: {
    ui: "bdd",
    timeout: 60000,
  },
  onPrepare: () => {
    fs.rmSync(WORKSPACE_ROOT, { recursive: true, force: true });
    ensurePodmanMachine(TEST_MACHINE);
    spawnSync("pnpm", ["tauri", "build", "--debug", "--no-bundle"], {
      cwd: path.resolve(__dirname, ".."),
      stdio: "inherit",
      shell: true,
    });
  },
  beforeSession: () => {
    tauriDriver = spawn(
      path.resolve(os.homedir(), ".cargo", "bin", `tauri-driver${ext}`),
      ["--native-driver", "../tools/msedgedriver.exe"],
      {
        stdio: [null, process.stdout, process.stderr],
        env: { ...process.env, PODMAN_CONNECTION: TEST_MACHINE },
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
  afterSession: () => {
    closeTauriDriver();
  },
  onComplete: () => {
    fs.rmSync(WORKSPACE_ROOT, { recursive: true, force: true });
  },
};

function closeTauriDriver() {
  exit = true;
  tauriDriver?.kill();
}

function onShutdown(fn) {
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
