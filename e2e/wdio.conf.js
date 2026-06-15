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
// info` verification step, and the same check the app makes in verify_host.
function waitForConnection(connectionName, timeoutMs = 15000) {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    if (spawnSync("podman", ["--connection", connectionName, "info"], { shell: true }).status === 0) {
      return;
    }
    if (Date.now() >= deadline) {
      throw new Error(`Podman connection "${connectionName}" did not become ready within ${timeoutMs}ms`);
    }
    sleepSync(500);
  }
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

  // Make the rootless Podman API socket available. This mirrors the tutorial's server-side
  // setup for connecting to an external Linux host:
  //   https://github.com/podman-container-tools/podman/blob/main/docs/tutorials/mac_win_client.md#creating-the-first-connection-to-an-external-linux-server
  // On a normal host that is:
  //   systemctl --user enable --now podman.socket   # start the API socket
  //   sudo loginctl enable-linger $USER             # keep it up without a login session
  // This podman machine is a minimal WSL image whose per-user systemd manager
  // (user@1000.service) fails to start, so `systemctl --user` has no session bus to reach.
  // We launch the rootless service directly instead — the same socket, without systemd.
  console.log(`Activating rootless Podman socket on "${machineName}"...`);
  // `nohup ... &` is insufficient on WSL: the background process is still in the SSH
  // session's process group and is killed when the connection closes. Wrapping in
  // `bash -c '... & disown'` removes the job from the shell's job table so it survives
  // the SSH teardown. The trailing `sleep 2` keeps the session open long enough for the
  // service to create the socket before waitForConnection starts polling.
  spawnSync(
    "podman",
    [
      "machine", "ssh", machineName,
      `bash -c 'test -S ${PODMAN_SOCKET} || { mkdir -p $(dirname ${PODMAN_SOCKET}) && XDG_RUNTIME_DIR=/run/user/1000 podman system service --time=0 &>/tmp/podman-svc.log & disown; sleep 2; }'`,
    ],
    { stdio: "inherit", shell: true }
  );

  // Verify the connection responds (tutorial: `podman --remote info`).
  console.log(`Waiting for the "${machineName}" connection to respond...`);
  waitForConnection(machineName);

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
