import os from "os";
import path from "path";
import { spawn, spawnSync } from "child_process";
import { fileURLToPath } from "url";

const __dirname = fileURLToPath(new URL(".", import.meta.url));

const TEST_MACHINE = "mio-tests";

let tauriDriver;
let exit = false;

const ext = process.platform === "win32" ? ".exe" : "";

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
        env: { ...process.env, PODMAN_MACHINE: `podman-${TEST_MACHINE}` },
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
