// `GITHUB_SERVICE` must equal the Rust one in src-tauri/src/credentials/mod.rs, and
// `GITHUB_AUTH_CONTAINER` the one in src-tauri/src/credentials/types.rs.
export const GITHUB_SERVICE = "github";
export const GITHUB_AUTH_CONTAINER = "mio-auth-github";

/** `start_agent_auth` prefixes `mise exec --`, which finds gh on PATH. */
export const GITHUB_LOGIN_COMMAND = [
    "gh",
    "auth",
    "login",
    "--hostname",
    "github.com",
    "--git-protocol",
    "https",
    "--web",
];

/** Pulls the device-flow one-time code out of one line of `gh auth login` output. */
export function parseDeviceCode(line: string): string | null {
    return line.match(/one-time code: ([A-Z0-9]{4}-[A-Z0-9]{4})/)?.[1] ?? null;
}
