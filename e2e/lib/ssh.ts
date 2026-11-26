import fs from "fs";
import os from "os";
import path from "path";

import { APP_DATA_ROOT } from "./run-identity.js";

/**
 * Substring of the spec path that makes `spawnDriver` (`e2e/wdio.conf.ts`) set `MIO_SSH_DIR`. The
 * spec asserts its own path contains it, so a rename fails loudly instead of letting the spec write
 * alias blocks into the real `~/.ssh`.
 */
export const SSH_DIR_SPEC_MARKER = "ssh-alias-shadow";

/**
 * The per-run SSH directory the harness hands to the shadow spec's app as `MIO_SSH_DIR` (gated in
 * `spawnDriver`, `e2e/wdio.conf.ts`). Host-global `MANAGED_SSH_CONFIG` stays as it is for every other spec.
 */
export const E2E_SSH_DIR = path.join(APP_DATA_ROOT, "ssh");

/** The app-owned OpenSSH config `crate::ssh::write_alias` writes (`src-tauri/src/ssh/mod.rs`). */
export const MANAGED_SSH_CONFIG = path.join(os.homedir(), ".ssh", "sandbox-mio", "config");

/** Mirrors `ALIAS_SUFFIX` in `src-tauri/src/ssh/identity.rs`; keep in step. */
export const ALIAS_SUFFIX = ".mio";

/** The SSH `Host` alias of sandbox `name`; mirrors `sandbox_alias` in `src-tauri/src/ssh/identity.rs`. */
export function sandboxAlias(name: string): string {
  return `${name}${ALIAS_SUFFIX}`;
}

/**
 * The marker-delimited `Host <alias>` block of `file`, exactly as `render_block` emits it, or
 * `undefined` when the file or the block is absent. Takes the literal alias, so a spec can also look
 * for a bare-name block, which no code path should write any more.
 */
export function readAliasBlock(alias: string, file: string = MANAGED_SSH_CONFIG): string | undefined {
  let contents: string;
  try {
    contents = fs.readFileSync(file, "utf8");
  } catch {
    return undefined;
  }
  const begin = `# >>> sandbox-mio ${alias} >>>`;
  const end = `# <<< sandbox-mio ${alias} <<<`;
  const from = contents.indexOf(begin);
  const to = contents.indexOf(end, from + begin.length);
  if (from === -1 || to === -1) return undefined;
  return contents.slice(from + begin.length, to).trim();
}

/**
 * The managed block of sandbox `name` in the host-global `MANAGED_SSH_CONFIG`, found under its
 * derived alias. Host-global rather than per-worktree, so a spec must key its assertion on a
 * run-unique sandbox name (or only assert absence).
 */
export function sshAliasBlock(name: string): string | undefined {
  return readAliasBlock(sandboxAlias(name));
}
