import { spawnSync } from "child_process";

// Podman connection names the specs target.
export const LOCAL_CONNECTION = "mio-tests";
export const REMOTE_CONNECTION = "mio-tests-remote";

// Run `podman --connection <connection> <...args>` and return trimmed stdout. Throws a diagnostic
// error naming the connection and args when check is true and the command exits non-zero; pass
// { check: false } for best-effort commands (e.g. cleanup) whose failure should be ignored.
export function podman(connection, args, { check = true } = {}) {
  const out = spawnSync("podman", ["--connection", connection, ...args], {
    encoding: "utf8",
    shell: false,
  });
  if (check && out.status !== 0) {
    throw new Error(
      `podman --connection ${connection} ${args.join(" ")} failed: ${out.stderr}`
    );
  }
  return out.stdout?.trim() ?? "";
}

// Returns true when the command exits 0 (e.g. the container/image exists), false otherwise.
export function podmanOk(connection, args) {
  return (
    spawnSync("podman", ["--connection", connection, ...args], { shell: false }).status === 0
  );
}
