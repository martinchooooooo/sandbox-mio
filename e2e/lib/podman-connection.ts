import { spawnSync } from "child_process";

export const LOCAL_CONNECTION = "mio-tests";
export const REMOTE_CONNECTION = "mio-tests-remote";

/** Returns trimmed stdout. Pass `{ check: false }` for best-effort commands such as cleanup. */
export function podman(
  connection: string,
  args: string[],
  { check = true }: { check?: boolean } = {}
): string {
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

/** True when the command exits 0 — e.g. `image exists`. */
export function podmanOk(connection: string, args: string[]): boolean {
  return (
    spawnSync("podman", ["--connection", connection, ...args], { shell: false }).status === 0
  );
}
