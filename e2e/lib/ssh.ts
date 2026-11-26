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

/**
 * The managed `Host <alias>` block, marker-delimited exactly as `render_block` emits it, or
 * `undefined` when no block for `alias` exists. Host-global rather than per-worktree, so a spec
 * must key its assertion on a run-unique alias.
 */
export function sshAliasBlock(alias: string): string | undefined {
  let contents: string;
  try {
    contents = fs.readFileSync(MANAGED_SSH_CONFIG, "utf8");
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
