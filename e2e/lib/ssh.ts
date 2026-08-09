import fs from "fs";
import os from "os";
import path from "path";

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
