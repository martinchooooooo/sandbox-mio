import { invoke } from "@tauri-apps/api/core";

/** What a host SSH client needs to reach a sandbox through its managed alias. Mirrors `SshAccess`
 *  in `src-tauri/src/ssh/types.rs`. */
export interface SshAccess {
    /** The `Host` token in `~/.ssh/sandbox-mio/config`, which is the sandbox name. */
    alias: string;
    identityPath: string;
    knownHostsPath: string;
    /** The exact `ProxyCommand` in the block; it names the sandbox's current machine, so never cache it. */
    proxyCommand: string;
    /** False for a stopped sandbox and for a deployed one; a client cannot connect to a stopped one. */
    running: boolean;
}

/** Rewrites the sandbox's alias block and returns its values. Never starts a stopped sandbox. */
export async function sandboxSshAccess(name: string): Promise<SshAccess> {
    return invoke<SshAccess>("sandbox_ssh_access", { name });
}
