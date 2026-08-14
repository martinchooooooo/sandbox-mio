// Per-worktree run identity: every path and port a run touches derives from one slug computed
// here, so a future collision has one place to be fixed (docs/features/parallel-worktree-e2e/02_dev.md).
import crypto from "crypto";
import fs from "fs";
import net from "net";
import os from "os";
import path from "path";
import { spawnSync } from "child_process";
import { fileURLToPath } from "url";

const __dirname = fileURLToPath(new URL(".", import.meta.url));

// This file lives at e2e/lib/, so the worktree root is two levels up.
export const WORKTREE_ROOT = path.resolve(__dirname, "..", "..");

// Characters illegal in a Windows path segment or filename, collapsed to `-`.
function sanitize(segment: string): string {
  return segment.replace(/[<>:"/\\|?*\s]+/g, "-").replace(/^-+|-+$/g, "") || "root";
}

// Path-derived rather than branch-derived, so the slug survives a branch rename/checkout and stays
// stable across runs in one worktree — onPrepare's destructive cleanup only ever reclaims its own
// worktree's leftovers. Exported as a callable so a test can apply it to a synthetic root.
export function deriveSlug(worktreeRoot: string): string {
  const digest = crypto.createHash("sha256").update(worktreeRoot).digest("hex").slice(0, 8);
  return `${sanitize(path.basename(worktreeRoot))}-${digest}`;
}

export const RUN_SLUG = deriveSlug(WORKTREE_ROOT);

export const APP_DATA_ROOT = path.join(os.homedir(), `.sandbox-mio-e2e-appdata-${RUN_SLUG}`);
export const WORKSPACE_ROOT = path.join(os.homedir(), `.sandbox-mio-e2e-${RUN_SLUG}`);

// microsandbox state root: `resolve_home()` upstream reads `MSB_HOME` as the root of *all* its
// state (db, sandboxes, cache, logs), so it must be slug-keyed or two worktrees' e2e runs collide on
// one shared installation (docs/features/microsandbox-provider/02_dev.md, "E2E isolation").
// `MSB_PATH` is pinned alongside it because a slug-keyed `MSB_HOME` holds no `bin/`, and it is the
// path to the `msb` executable itself — `msb` resolves its own VM supervisor from the inherited
// value, and a directory there fails every `msb create` (`msb_command()` in
// `src-tauri/src/mockctl/mod.rs`).
export const MSB_HOME = path.join(os.homedir(), `.sandbox-mio-e2e-msb-${RUN_SLUG}`);
export const MSB_PATH =
  process.env.MSB_PATH ??
  path.join(
    os.homedir(),
    ".microsandbox",
    "bin",
    `msb${process.platform === "win32" ? ".exe" : ""}`,
  );

// Trailing separator: a bare prefix match (`StartsWith`) would otherwise also match a sibling
// worktree whose absolute path happens to start with this one's (e.g. a nested `.claude/worktrees/*`
// checkout, or two worktree names sharing a prefix).
export const DEBUG_DIR = path.join(WORKTREE_ROOT, "src-tauri", "target", "debug") + path.sep;

// e2e/wdio.conf.ts is re-evaluated once per process: the launcher, and again in every worker
// @wdio/runner forks (its own ConfigParser per worker). Only the launcher lacks WDIO_WORKER_ID.
// Every side effect below (locks, port allocation) must stay behind this, or a worker re-runs it.
export const IS_LAUNCHER = !process.env.WDIO_WORKER_ID;

const WORKTREE_LOCK_PATH = path.join(APP_DATA_ROOT, "run.lock");
const HOST_LOCK_PATH = path.join(os.homedir(), ".sandbox-mio-e2e-host.lock");

interface LockHolder {
  pid: number;
  startTime: string;
  worktree: string;
}

// "gone" is the only verdict that lets a lock be reclaimed: an unreadable pid must read as live, or
// a run that cannot be inspected has its lock stolen mid-flight.
type Liveness = "live" | "gone" | "unknown";

interface ProcessInfo {
  exists: boolean;
  startTime?: string;
}

// Windows-only: queries the process-creation timestamp for `pid`, so a pid reused by an unrelated
// process after the holder exited is never mistaken for the holder (Windows recycles process ids).
// `Get-CimInstance Win32_Process` rather than `(Get-Process -Id).StartTime`, which throws for a
// process whose token this session cannot open — an empty stdout that is indistinguishable from
// "no such pid", so a live holder would be judged stale. CIM exposes `CreationDate` without
// process-handle rights. `undefined` means the query itself failed and nothing can be concluded.
function queryProcess(pid: number): ProcessInfo | undefined {
  if (process.platform !== "win32") return undefined;
  const result = spawnSync(
    "powershell",
    [
      "-NoProfile",
      "-Command",
      `$p = Get-CimInstance Win32_Process -Filter "ProcessId=${pid}" -ErrorAction SilentlyContinue; ` +
        'if ($null -eq $p) { "gone" } else { "exists:" + $p.CreationDate.Ticks }',
    ],
    { encoding: "utf8" },
  );
  const out = result.stdout?.trim();
  if (!out) return undefined;
  if (out === "gone") return { exists: false };
  const ticks = out.startsWith("exists:") ? out.slice("exists:".length).trim() : "";
  return { exists: true, startTime: ticks || undefined };
}

function ownStartTime(): string {
  return queryProcess(process.pid)?.startTime ?? "";
}

function holderLiveness(holder: LockHolder): Liveness {
  if (process.platform !== "win32") {
    // Existence only — there is no start time to compare against, so a pid reused by an unrelated
    // process after the holder exited reads as live here. The suite is Windows-first; this keeps
    // the lock a real (if conservative) lock off Windows rather than a silent no-op.
    try {
      process.kill(holder.pid, 0);
      return "live";
    } catch (e) {
      return (e as NodeJS.ErrnoException).code === "ESRCH" ? "gone" : "unknown";
    }
  }
  const info = queryProcess(holder.pid);
  if (info === undefined) return "unknown";
  if (!info.exists) return "gone";
  // The pid is taken but its creation time is unreadable: cannot rule out that this is the holder.
  if (info.startTime === undefined || !holder.startTime) return "unknown";
  return info.startTime === holder.startTime ? "live" : "gone";
}

// `wx` rather than existsSync + writeFileSync: the check-then-write pair let two launchers starting
// in the same tick both see no lock and both write, and the second silently won.
function createLockExclusive(lockPath: string): boolean {
  const holder: LockHolder = {
    pid: process.pid,
    startTime: ownStartTime(),
    worktree: WORKTREE_ROOT,
  };
  let fd: number;
  try {
    fd = fs.openSync(lockPath, "wx");
  } catch (e) {
    if ((e as NodeJS.ErrnoException).code === "EEXIST") return false;
    throw e;
  }
  try {
    fs.writeFileSync(fd, JSON.stringify(holder));
  } finally {
    fs.closeSync(fd);
  }
  return true;
}

function readHolder(lockPath: string): LockHolder | undefined {
  try {
    return JSON.parse(fs.readFileSync(lockPath, "utf8")) as LockHolder;
  } catch {
    // Truncated or absent: a crash between the create and the write, or a release that raced this
    // read. Nothing to protect, so treat it as reclaimable.
    return undefined;
  }
}

function acquireLock(lockPath: string): void {
  fs.mkdirSync(path.dirname(lockPath), { recursive: true });
  if (createLockExclusive(lockPath)) return;

  const holder = readHolder(lockPath);
  if (holder && holderLiveness(holder) !== "gone") {
    throw new Error(
      `[e2e] lock ${lockPath} is already held by process ${holder.pid} in worktree ` +
        `${holder.worktree}. Wait for that run to finish, or remove the file if it crashed ` +
        "without releasing the lock.",
    );
  }
  // The recorded pid is gone, or was reused by an unrelated process: stale, reclaim it. Exactly one
  // retry — a second EEXIST means another launcher won the reclaim race, and looping would let the
  // two take turns stealing the lock from each other.
  fs.rmSync(lockPath, { force: true });
  if (createLockExclusive(lockPath)) return;
  throw new Error(
    `[e2e] lock ${lockPath} was claimed by another run while this one was reclaiming it as stale. ` +
      "Re-run once that run finishes.",
  );
}

// Only the process that acquired a lock may release it. Without this, a run *refused* by
// acquireLock() still ran the IS_LAUNCHER-gated release path on its way out (e2e/wdio.conf.ts's
// onShutdown), deleting the incumbent's lock file and letting a third run start on top of it.
let heldWorktreeLock = false;
let heldHostLock = false;

// One light run per worktree at a time — a second run in the same worktree would otherwise share
// ports, workspace root, and app-data root with the first and silently corrupt it.
export function acquireWorktreeLock(): void {
  fs.mkdirSync(APP_DATA_ROOT, { recursive: true });
  acquireLock(WORKTREE_LOCK_PATH);
  heldWorktreeLock = true;
}
export function releaseWorktreeLock(): void {
  if (!heldWorktreeLock) return;
  heldWorktreeLock = false;
  fs.rmSync(WORKTREE_LOCK_PATH, { force: true });
}

// One standard/heavy run per host — mio-tests/mio-tests-remote and ~/.ssh cannot be slug-keyed, so
// a second real-infra run is refused rather than isolated (02_dev.md, Resolved Decision 5).
export function acquireHostLock(): void {
  acquireLock(HOST_LOCK_PATH);
  heldHostLock = true;
}
export function releaseHostLock(): void {
  if (!heldHostLock) return;
  heldHostLock = false;
  fs.rmSync(HOST_LOCK_PATH, { force: true });
}

// ---- Port pair allocation (Phase E) ----

const DEFAULT_PORT_BASE = 45000;
const PORT_BAND_SIZE = 1000;
export const PORT_WINDOW_SIZE = 16;
const PORT_WINDOWS = Math.floor(PORT_BAND_SIZE / PORT_WINDOW_SIZE);

export const MIO_PORT_BASE = Number(process.env.MIO_PORT_BASE ?? DEFAULT_PORT_BASE);

function digestToInt(value: string): number {
  return parseInt(crypto.createHash("sha256").update(value).digest("hex").slice(0, 8), 16);
}

// Windows are disjoint for distinct digests but not guaranteed disjoint across worktrees — a
// digest reduced into a finite set collides at a birthday rate (02_dev.md, Resolved Decision 10).
// A strong default, not a proof; allocatePortPair()'s probing and the caller's one respawn retry
// are what make an actual collision recoverable rather than fatal.
export const PORT_WINDOW_INDEX = digestToInt(RUN_SLUG) % PORT_WINDOWS;
export const PORT_WINDOW_BASE = MIO_PORT_BASE + PORT_WINDOW_INDEX * PORT_WINDOW_SIZE;

// Probes 127.0.0.1 only, matching where tauri-driver binds — a listener on 0.0.0.0 holding the same
// port is not detected, so this is a filter and not an authoritative answer. What covers the miss is
// e2e/wdio.conf.ts's spawnTauriDriver: it waits for the driver to actually start listening and
// respawns on a fresh pair if it never does.
function isPortFree(port: number): Promise<boolean> {
  return new Promise((resolve) => {
    const server = net.createServer();
    server.once("error", () => resolve(false));
    server.listen(port, "127.0.0.1", () => {
      server.close(() => resolve(true));
    });
  });
}

// Probes upward inside this worktree's own window for two consecutive free ports (driver, native).
// Probing rather than a fixed offset absorbs a re-run started while the previous run's ports are
// still in TIME_WAIT; the window keeps two worktrees from ever probing the same numbers except at
// the rare digest collision above.
export async function allocatePortPair(): Promise<{ driverPort: number; nativePort: number }> {
  for (let offset = 0; offset < PORT_WINDOW_SIZE - 1; offset++) {
    const driverPort = PORT_WINDOW_BASE + offset;
    const nativePort = driverPort + 1;
    if ((await isPortFree(driverPort)) && (await isPortFree(nativePort))) {
      return { driverPort, nativePort };
    }
  }
  throw new Error(
    `[e2e] no free port pair in window ${PORT_WINDOW_BASE}-${PORT_WINDOW_BASE + PORT_WINDOW_SIZE - 1} ` +
      `(MIO_PORT_BASE=${MIO_PORT_BASE}). Set MIO_PORT_BASE to move the band.`,
  );
}
