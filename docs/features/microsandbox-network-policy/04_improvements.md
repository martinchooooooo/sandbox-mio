# Microsandbox Network Policy: Deferred Improvements

Ideas scoped out of `docs/features/microsandbox-network-policy/01_ideas.md` as worthwhile but outside the native-policy-plus-chained-proxy feature.

## Request-level rules in the chained mitmproxy

Allow or deny by HTTP method and path (for example, read-only access to an API) in a mitmproxy addon loaded into the chained proxy. Every microsandbox request already passes through mitmproxy once the chained proxy exists, so the addon is a small addition, and its rules reload by restarting only the proxy, without recreating the sandbox. Deferred until the chained proxy and TLS layering clear the `01_ideas.md` Validation spike. `proxy-network-policy`'s `mitm_policy_addon.py` design (`docs/features/proxy-network-policy/02_dev.md`) is the starting point.

## Interactive approval for microsandbox sandboxes

Prompt the user when a microsandbox sandbox is denied a new host, then add the host to `.mio/net-policy.json`. Microsandbox's JSON `403` denial response (`code`, `message`, `domain`) gives an agent-readable signal, and `network_deny_message` can tell the agent to ask. Applying an approved host still needs a sandbox recreate, because `msb modify` cannot change network rules. Deferred because the approval channel is already designed for Podman in `proxy-network-policy` Phases D–F and should be shared, not duplicated.

## Adopting NVIDIA OpenShell

NVIDIA OpenShell (github.com/NVIDIA/OpenShell, read at commit `42671c2`, 2026-10-31) mediates every TCP connection and DNS query through a trusted supervisor outside a no-network workload. Its unique value over `01_ideas.md`'s design is:

- rules per program (real executable path, hash-pinned, inherited by child processes);
- live network-policy reload with no sandbox restart;
- an agent-proposes, human-approves policy workflow;
- a formal policy prover;
- a structured audit stream over gRPC (`GetSandboxLogs`).

Deferred because on this project's Windows host OpenShell runs only inside WSL 2, which OpenShell marks experimental. Its Podman driver needs a `systemd --user` Podman socket the podman machine lacks. It also needs Landlock ABI 3 and seccomp user-notification in the WSL kernel. Processes started by `podman exec` get no egress, which breaks the exec transport (`docs/features/podman-exec-transport/02_dev.md`). Revisit if rules per program become a requirement. The cheapest test is OpenShell running inside a microsandbox VM built from this project's base image: confirm the OpenShell kernel qualification passes, an SSH terminal session gets mediated egress, and the per-sandbox memory overhead is acceptable. OpenShell's policy semantics (every matching rule adds access, deny wins regardless of order, an audit mode before enforcement) are worth copying into `.mio/net-policy.json` regardless.

## Rate and connection limits

Microsandbox 0.6.9 and 0.7.0 added per-sandbox egress/ingress bandwidth and packet budgets plus TCP/UDP connection caps. Exposing them in `.mio/net-policy.json` would cap a runaway agent's bandwidth. Deferred because no user has asked for limits and the defaults are unlimited for single-tenant sandboxes.

## Live secret rotation through `msb modify --secret`

Push a changed secret value into a running microsandbox sandbox with `msb modify <name> --secret ENV[:OPTIONS]@HOST[,HOST...]`, the value supplied in the `modify` process environment (`spike/02_secrets.md`, Item 4: value rotation and host-list changes apply live). Deferred because microsandbox already re-reads every value at each start (`02_dev.md`, Resolved Decision 16), so a rotated value takes effect at the next start with no extra call. Live rotation needs `set_proxy_secret` to find every running sandbox bound to the secret id and repeat each binding's full host list, which is new fan-out logic.

## Automated `msb self update`

Let the microsandbox setup flow (`MsbMachineManager::prepare`, `src-tauri/src/machine/msb.rs`) update an outdated `msb` itself by copying `msb.exe` to a temporary path and running `<copy> self update`, the only form that works on Windows. Deferred because `02_dev.md` Phase A only reports the version gap with instructions (Resolved Decision 14): microsandbox is a system-installed prerequisite the app has never modified, and an update interrupted mid-run could leave the user without a working `msb`.

## Effective-policy readback from `msb inspect`

Show the policy microsandbox actually enforces, read from `msb inspect <name> --format json` at `active_config.network.policy` and `active_config.network.http.deny_response` (`spike/01_egress-baseline.md`), beside the workspace file in `ProxyPanel.svelte`. Deferred because `02_dev.md` detects drift against the app's own `applied.json` snapshot (Resolved Decision 18), which covers the "changes pending" need without parsing microsandbox's inspect schema, and that schema is not documented as stable.

## Agent-facing deny message

Render `.mio/net-policy.json`'s `denyMessage` into microsandbox's `network_deny_message` so a blocked agent tells the user which host to add. `msb create --help` on 0.7.12 exposes no flag for it; the only route is `--net-conf <PATH>`, an "unwrapped network configuration" file whose schema is undocumented and would replace the `--net-*` flags `src-tauri/src/provider/msb_policy.rs` renders today. Deferred until microsandbox documents `--net-conf` or adds a flag; the field stays in the schema and in `EgressPolicy` so the editor and the file need no change then.

## UDP behaviour behind the chained viewer

Confirm whether non-DNS UDP from a chained microsandbox agent is dropped or sent directly, bypassing the mitmproxy sidecar (SOCKS5 mitmproxy has no UDP). `02_dev.md` Phase F could not test it because `sandbox-mio/base` ships no UDP client (no `nc`, `dig`, `node` or `python3` on `PATH` at create). Deferred because the viewer's documented scope is already TCP/HTTPS only; the check needs a tool installed through mise first, which belongs in the standard-tier spec.
