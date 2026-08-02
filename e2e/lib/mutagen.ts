import fs from "fs";
import os from "os";
import path from "path";
import { spawnSync, type SpawnSyncReturns } from "child_process";
import { fileURLToPath } from "url";

const __dirname = fileURLToPath(new URL(".", import.meta.url));

// Must match the app's sidecar mutagen, so the CLI and the app share one daemon.
const MUTAGEN_VERSION = "v0.18.1";
const TOOLS_DIR = path.resolve(__dirname, "../../tools");
const MUTAGEN_BIN = path.join(TOOLS_DIR, "mutagen.exe");

/** Windows-only: the suite targets a Windows laptop driving Podman machines. */
export function ensureMutagen(): string {
  if (fs.existsSync(MUTAGEN_BIN)) {
    return MUTAGEN_BIN;
  }
  if (process.platform !== "win32") {
    throw new Error("ensureMutagen only downloads the Windows build; install mutagen manually on this platform");
  }
  fs.mkdirSync(TOOLS_DIR, { recursive: true });
  const asset = `mutagen_windows_amd64_${MUTAGEN_VERSION}.zip`;
  const url = `https://github.com/mutagen-io/mutagen/releases/download/${MUTAGEN_VERSION}/${asset}`;
  console.log(`Downloading Mutagen ${MUTAGEN_VERSION}...`);
  const ps = spawnSync(
    "powershell",
    [
      "-NoProfile", "-Command",
      `$ProgressPreference='SilentlyContinue'; $z="$env:TEMP\\${asset}"; ` +
      `Invoke-WebRequest -Uri "${url}" -OutFile $z; ` +
      `Expand-Archive -Path $z -DestinationPath "${TOOLS_DIR}" -Force`,
    ],
    { stdio: "inherit", shell: false }
  );
  if (ps.status !== 0 || !fs.existsSync(MUTAGEN_BIN)) {
    throw new Error("failed to download Mutagen");
  }
  return MUTAGEN_BIN;
}

function mutagen(args: string[]): SpawnSyncReturns<string> {
  const out = spawnSync(MUTAGEN_BIN, args, { encoding: "utf8", shell: false });
  return out;
}

/** Two-way-resolved between a local directory (alpha) and an `alias:path` remote (beta). */
export function syncCreate(name: string, alpha: string, beta: string): void {
  mutagen(["sync", "terminate", name]); // idempotent across runs; ignore "no such session"
  const out = mutagen(["sync", "create", `--name=${name}`, "--sync-mode=two-way-resolved", alpha, beta]);
  if (out.status !== 0) {
    throw new Error(`mutagen sync create failed: ${out.stderr || out.stdout}`);
  }
}

/** Blocks until reconcile finishes, so assertions do not race the watcher's debounce. */
export function syncFlush(name: string): void {
  const out = mutagen(["sync", "flush", name]);
  if (out.status !== 0) {
    throw new Error(`mutagen sync flush failed: ${out.stderr || out.stdout}`);
  }
}

export function syncTerminate(name: string): void {
  mutagen(["sync", "terminate", name]);
}

// Mirrors the MUTAGEN_DATA_DIRECTORY the app sets. Helpers that touch the app's sessions must pass
// it, or the CLI talks to a different daemon than the app.
const APP_MUTAGEN_DIR = path.join(
  process.env.APPDATA ?? path.join(os.homedir(), "AppData", "Roaming"),
  "com.tincho.sandbox-mio",
  "mutagen"
);

/** Combined stdout+stderr of `mutagen forward list`, or empty when the session does not exist. */
export function forwardList(name: string): string {
  const out = spawnSync(MUTAGEN_BIN, ["forward", "list", name], {
    encoding: "utf8",
    shell: false,
    env: { ...process.env, MUTAGEN_DATA_DIRECTORY: APP_MUTAGEN_DIR },
  });
  return ((out.stdout ?? "") + (out.stderr ?? "")).trim();
}

// Names are load-bearing: the startup sweep must terminate the orphan's prefix and leave the
// other alone (docs/features/agent-web-ui/03_e2e.md).
export const SWEEP_ORPHAN_SESSION = "agentweb-e2e-orphan-sweep";
export const SWEEP_KEEP_SESSION = "mio-e2e-keep-sweep";

/** Seam for planting sessions in the app's private daemon before the app process spawns. */
export function forwardCreate(name: string, source: string, dest: string): void {
  const env = { ...process.env, MUTAGEN_DATA_DIRECTORY: APP_MUTAGEN_DIR };
  spawnSync(MUTAGEN_BIN, ["forward", "terminate", name], { encoding: "utf8", shell: false, env });
  const out = spawnSync(MUTAGEN_BIN, ["forward", "create", `--name=${name}`, source, dest], {
    encoding: "utf8",
    shell: false,
    env,
  });
  if (out.status !== 0) {
    throw new Error(`mutagen forward create ${name} failed: ${out.stderr || out.stdout}`);
  }
}

/** Best-effort, like syncTerminate. */
export function forwardTerminate(name: string): void {
  spawnSync(MUTAGEN_BIN, ["forward", "terminate", name], {
    encoding: "utf8",
    shell: false,
    env: { ...process.env, MUTAGEN_DATA_DIRECTORY: APP_MUTAGEN_DIR },
  });
}

// Aliases exist because Mutagen's scp-style URLs cannot carry a port and the Podman machine's SSH
// port is dynamic. Marker-delimited so a rewrite never disturbs the user's own config.
const SSH_CONFIG = path.join(os.homedir(), ".ssh", "config");

export interface SshEndpoint {
  host: string;
  port: string | number;
  user: string;
  identity: string;
}

function markers(alias: string): [string, string] {
  return [`# >>> mio-e2e ${alias} >>>`, `# <<< mio-e2e ${alias} <<<`];
}

function stripBlock(text: string, alias: string): string {
  const [begin, end] = markers(alias);
  const lines = text.split(/\r?\n/);
  const out: string[] = [];
  let inside = false;
  for (const line of lines) {
    if (line.trim() === begin) { inside = true; continue; }
    if (line.trim() === end) { inside = false; continue; }
    if (!inside) out.push(line);
  }
  return out.join("\n");
}

export function writeSshAlias(alias: string, { host, port, user, identity }: SshEndpoint): void {
  fs.mkdirSync(path.dirname(SSH_CONFIG), { recursive: true });
  const existing = fs.existsSync(SSH_CONFIG) ? fs.readFileSync(SSH_CONFIG, "utf8") : "";
  const [begin, end] = markers(alias);
  const block = [
    begin,
    `Host ${alias}`,
    `    HostName ${host}`,
    `    Port ${port}`,
    `    User ${user}`,
    `    IdentityFile ${identity}`,
    "    IdentitiesOnly yes",
    "    StrictHostKeyChecking accept-new",
    end,
  ].join("\n");
  const base = stripBlock(existing, alias).replace(/\s+$/, "");
  fs.writeFileSync(SSH_CONFIG, `${base}\n${block}\n`, "ascii");
}

export function removeSshAlias(alias: string): void {
  if (!fs.existsSync(SSH_CONFIG)) return;
  const stripped = stripBlock(fs.readFileSync(SSH_CONFIG, "utf8"), alias).replace(/\s+$/, "");
  fs.writeFileSync(SSH_CONFIG, stripped ? `${stripped}\n` : "", "ascii");
}
