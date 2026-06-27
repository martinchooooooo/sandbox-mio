import fs from "fs";
import os from "os";
import path from "path";
import { spawnSync, type SpawnSyncReturns } from "child_process";
import { fileURLToPath } from "url";

const __dirname = fileURLToPath(new URL(".", import.meta.url));

// Mutagen is vendored under the repo-root tools/ directory alongside msedgedriver, pinned to a
// known-good version so test runs are reproducible.
const MUTAGEN_VERSION = "v0.18.1";
const TOOLS_DIR = path.resolve(__dirname, "../../tools");
const MUTAGEN_BIN = path.join(TOOLS_DIR, "mutagen.exe");

// Download and unpack the pinned Mutagen if it is not already vendored. Windows-only: the suite
// targets a Windows laptop driving Podman machines.
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

// Create a two-way-resolved sync session between a local directory (alpha) and an
// `alias:path` remote (beta). Terminates any prior session of the same name first so the call
// is idempotent across runs.
export function syncCreate(name: string, alpha: string, beta: string): void {
  mutagen(["sync", "terminate", name]); // ignore "no such session"
  const out = mutagen(["sync", "create", `--name=${name}`, "--sync-mode=two-way-resolved", alpha, beta]);
  if (out.status !== 0) {
    throw new Error(`mutagen sync create failed: ${out.stderr || out.stdout}`);
  }
}

// Force a synchronous reconcile and wait for it to finish, so assertions do not race the
// filesystem watcher's debounce.
export function syncFlush(name: string): void {
  const out = mutagen(["sync", "flush", name]);
  if (out.status !== 0) {
    throw new Error(`mutagen sync flush failed: ${out.stderr || out.stdout}`);
  }
}

export function syncTerminate(name: string): void {
  mutagen(["sync", "terminate", name]);
}

// App-private Mutagen data directory. The Tauri app sets MUTAGEN_DATA_DIRECTORY to
// app_data_dir()/mutagen (%APPDATA%\com.tincho.sandbox-mio\mutagen) so the daemon is isolated
// from any user-installed Mutagen. Test helpers that query the app's sessions must pass this
// directory so the CLI client connects to the same daemon.
const APP_MUTAGEN_DIR = path.join(
  process.env.APPDATA ?? path.join(os.homedir(), "AppData", "Roaming"),
  "com.tincho.sandbox-mio",
  "mutagen"
);

// Returns the combined stdout+stderr of `mutagen forward list <name>` against the app's private
// daemon, or an empty string when the session does not exist (command exits non-zero). Use to
// assert a session is "Connected" or that it is absent.
export function forwardList(name: string): string {
  const out = spawnSync(MUTAGEN_BIN, ["forward", "list", name], {
    encoding: "utf8",
    shell: false,
    env: { ...process.env, MUTAGEN_DATA_DIRECTORY: APP_MUTAGEN_DIR },
  });
  return ((out.stdout ?? "") + (out.stderr ?? "")).trim();
}

// ---- SSH host alias management -------------------------------------------------------------
//
// Mutagen uses the system OpenSSH client and reads ~/.ssh/config, but its scp-style URLs cannot
// carry a port. The Podman machine's SSH port is dynamic, so each run writes a managed Host
// block naming the current port and the Podman-generated identity key, then removes it on
// teardown. The block is delimited by markers so it can be rewritten or removed cleanly without
// disturbing the user's own config.

const SSH_CONFIG = path.join(os.homedir(), ".ssh", "config");

// The SSH endpoint a managed Host alias points at.
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

// Write (or replace) a managed Host alias pointing at the given SSH endpoint and identity.
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
