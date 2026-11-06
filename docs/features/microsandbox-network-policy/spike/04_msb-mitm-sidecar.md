---
type: spike
title: "Spike 04: mitmproxy as a Microsandbox Sidecar on msb 0.7.12"
feature: microsandbox-network-policy
---
# Spike 04: mitmproxy as a Microsandbox Sidecar on msb 0.7.12

Hands-on spike run 2026-11-02 on the Windows 11 development host against `msb 0.7.12`, using `docker.io/mitmproxy/mitmproxy:11.1.3` (the `MITMPROXY_IMAGE_DEFAULT` in `src-tauri/src/proxy/mod.rs`) and the agent image `sandbox-mio/base:6`. It validates the user decision that mitmproxy for a microsandbox sandbox runs in its own microsandbox VM (not Windows-native), chained from the agent sandbox through a SOCKS5 port published to host loopback. It supersedes the "Windows-native `mitmdump`" recommendation of `spike/03_chained-proxy.md`. Podman was down; nothing here touched it. All commands ran from Git Bash with `MSYS_NO_PATHCONV=1`, host paths passed through `cygpath -m`. All `spike-netpol-side-*` sandboxes and the volume `spike-netpol-side-vol` were removed; no image tag was created. The pulled `docker.io/mitmproxy/mitmproxy:11.1.3` image was kept. Temp files lived under `C:\Users\krist\.claude\jobs\c32fd5a9\tmp\s4`.

## Result Table

| # | Check | Result | Evidence |
|---|---|---|---|
| 1 | `msb pull` of the Podman-path mitmproxy image | PASS | `msb pull docker.io/mitmproxy/mitmproxy:11.1.3` took 8.3 s, 98.2 MiB; no `msb load` needed |
| 1b | Boot and footprint | PASS | `msb create` returns in about 1.0 s; `msb start` of a stopped sidecar 0.7 s; SOCKS listener serving a chained request about 3.4 s after `start`; idle 113 to 121 MiB RSS of the default 512 MiB, 1 vCPU |
| 2a | mitmdump `--mode socks5@0.0.0.0:1080 --proxyauth` with the unmodified flow-log addon inside the VM | PASS | listener up, chained flows logged |
| 2b | Addons via `--mount-dir ...:/addons:ro` | PASS | addon loaded; guest cannot write the mount |
| 2c | Flows dir via `--mount-dir` rw, tailed live from Windows | PASS | row visible on the host immediately after the guest request; 505-request burst triggered the addon's 500-line truncate-and-rewrite through the mount, host saw exactly 500 lines |
| 2d | Single file via `--mount-file ...:/addons/flows.ndjson` | PASS | guest appends visible on the host; keeps the existing `FLOWS_PATH` constant; truncate-by-rewrite not exercised on a file mount |
| 2e | Named volume + `msb cp` | PARTIAL | works (`msb cp <sandbox>:/flows/mitmproxy-ca-cert.pem <host path>`), but is a snapshot copy, not a live tail; the volume is also a plain host directory under `~/.microsandbox/volumes/<name>`, an msb-internal path |
| 2f | `--copy-dir` for addons | FAIL (silent) | files land with mtime 1970; mitmdump loaded and served but the addon never fired and no flow row was written, no error. Same addon via `--mount-dir` fired. Cause not isolated (suspected epoch mtime vs mitmproxy's script reload check) |
| 3 | CA reachable from the host and stable across recreate | PASS | `/home/mitmproxy/.mitmproxy` bind-mounted from a host dir: `mitmproxy-ca-cert.pem` appeared on the host at first start; remove plus recreate on the same dir kept the same md5 and the agent kept working with the same `--tls-upstream-ca-cert` |
| 4a | Permissive agent (default egress, `--tls-intercept`, `--proxy socks5://127.0.0.1:<hostport>` + auth + `--tls-upstream-ca-cert`) | PASS | `curl --noproxy '*' https://httpbin.org/get` 200, `git ls-remote https://github.com/jdx/mise HEAD`, npm registry 200; `flows.ndjson` rows with method, host, path, status (rows below) |
| 4b | Deny-egress agent (`--net-default-egress deny` + allow rules + `--net-strict`) | PASS | allowed host 200 with a flow row; non-allowed `example.com` `Could not resolve host` |
| 4c | Permissive agent with `--proxy` but no `--tls-intercept` | PARTIAL | HTTPS: curl `(60) unable to get local issuer certificate` (the guest receives mitmproxy's CA-signed leaf, issuer `CN = mitmproxy`, which it does not trust); no flow row. Plain HTTP: row with full path. With `curl -k` HTTPS rows appear with paths, but only because the guest accepts an untrusted CA |
| 5a | Published port bind address | PASS | `netstat -ano`: `TCP 127.0.0.1:19081 ... LISTENING`; no `0.0.0.0` or LAN listener |
| 5b | Agent reaches sidecar some other way | PASS (not by default) | default agent: `host.microsandbox.internal` resolves to `172.16.0.9` but `:19081` fails `Couldn't connect`. Agent with `--net public,host`: reaches the port, `--socks5` without credentials `No authentication method was acceptable`, with credentials works |
| 6a | Sidecar stopped | PASS (fail closed) | agent `curl: (7) Failed to connect to httpbin.org port 443 after 8056 ms`; no direct bypass |
| 6b | Sidecar restart recovers the chain | PASS | agent sandbox never restarted; the next request after the sidecar came back returned 200 |
| 6c | Sidecar graceful stop with `--init` | FAIL | `msb stop` hangs in `draining` indefinitely (even `msb stop -t 20` times out, "no kill was requested"), also with a `trap ... TERM` wrapper. `msb stop -f` stops in 0.08 s with no data loss seen |
| 6d | Sidecar with `--net-default-egress deny` | FAIL (expected) | sidecar's own DNS fails (`Name or service not known`); the chain fails for the agent. The sidecar needs egress; msb's default (implicit `allow@public`) is enough |
| 7 | Two pairs concurrently on ports 19081 and 19082 | PASS | each request landed only in its own `flows.ndjson`, own passwords, own CA |

## Item 1: Image, Boot, Footprint

`msb pull docker.io/mitmproxy/mitmproxy:11.1.3` pulled from Docker Hub directly (8.3 s, 98.2 MiB). It is listed by `msb image ls` under the full reference, so `msb create` must be given the fully qualified `docker.io/mitmproxy/mitmproxy:11.1.3` (the same string `mitmproxy_image()` returns). `msb metrics <name>`: `118.2 MiB / 512.0 MiB` idle after the first connection, `112.8` to `121.0 MiB` in other runs, 0 to 0.08 CPU. The 512 MiB / 1 vCPU defaults were enough; `--memory` can shrink it (not tried).

## Item 2: Running mitmdump in the VM

**The image's CMD does not run.** `msb create IMAGE ... -- args` is rejected (`error: unexpected argument '--mode' found`); `create` accepts no command. `--entrypoint <EXECUTABLE>` takes only an executable, and an entrypoint set that way did not start anything on `create` (the sandbox idles with agentd as PID 1). The working patterns:

- Launch script plus `--init` (recommended): `--script 'start=mitmdump --set confdir=/home/mitmproxy/.mitmproxy --mode socks5@0.0.0.0:1080 --proxyauth u:p -s /addons/flowlog_addon.py' --init /.msb/scripts/start`. `--script NAME=BODY` writes an executable `/.msb/scripts/<name>`; `--init` hands PID 1 to it, so mitmdump starts on every `create` and every `start` with no further call. `msb ls` shows the sandbox `running` and the published port is bound at once, but the listener is up only a few seconds later (about 3.4 s measured from `start` to the first successful chained request).
- Plain script plus `msb exec -d <sidecar> -- /.msb/scripts/start`: works (returns `job_...`), but a stop/start loses the job, so the app would have to re-exec after every start. After `start` without re-exec the port is published but nothing listens (agent sees `(7)`).

**Stop caveat of `--init`.** With mitmdump as the init child, `msb stop` never completes (state `draining`); a `trap "kill $P" TERM INT; wait $P` wrapper did not help. Use `msb stop -f` for sidecars (stateless; the addon appends and closes per row). The no-`--init` variant stopped gracefully in under a second.

**Console output.** mitmdump's stdout is not shown by `msb logs` under `--init`. Redirecting inside the script (`> /flows/out.log 2>&1`) gave a readable console log on the host; worth keeping for debugging.

**Delivery mechanisms compared**

| Mechanism | Live host read | Survives sidecar recreate | Works | Notes |
|---|---|---|---|---|
| `--mount-dir HOST:GUEST[:ro]` | yes, immediate | yes (host dir) | yes | Addons `:ro` verified. statx ELOOP not hit. Guest root-written files appear as the host user |
| `--mount-file HOST:GUEST` | yes | yes | yes | Host file must exist first (the app already creates `flows.ndjson` if absent); lets the existing `/addons/flows.ndjson` path stay; rewrite-truncate not tested on a file mount |
| `--mount-named VOL:GUEST` + `msb cp` | no (poll by copy) | yes | yes | Extra `volume create` / `volume rm` lifecycle; not tail-able without reaching into `~/.microsandbox/volumes` |
| `--copy-dir HOST:GUEST` | n/a | n/a | addon silently inert | mtime 1970 files; do not use for addons |

**Recommendation:** `--mount-dir` for everything: addons dir read-only, a flows dir read-write, a confdir read-write. The Podman path mounts single files (`rules.json`, `flows.ndjson`) at fixed `/addons/...` paths; `--mount-file` can replicate that 1:1, but a read-only `/addons` dir plus a separate rw `/flows` dir is simpler and was fully verified (including truncate). If kept as files, change the constant `FLOWS_PATH` in `mitm_flowlog_addon.py` or keep `--mount-file` for `flows.ndjson` only. Windows reads of `flows.ndjson` while the guest appends worked on every poll in this spike; the existing `flowlog.rs` tail logic needs no change.

## Item 3: CA Certificate

With `--mount-dir <host confdir>:/home/mitmproxy/.mitmproxy` (rw) and mitmdump `--set confdir=/home/mitmproxy/.mitmproxy`, the first start writes `mitmproxy-ca-cert.pem`, `mitmproxy-ca.pem`, `.cer`, `.p12`, `mitmproxy-dhparam.pem` to the host dir within about a second of the listener starting. Removing and recreating the sidecar on the same host dir reused the CA (same md5; the existing agent sandbox kept its `--tls-upstream-ca-cert` and kept working). This is also exactly the layout `create_proxy` uses for `certs/` under the Podman sidecar, so `wait_for_ca` (polls for `certs/mitmproxy-ca-cert.pem`) carries over unchanged.

**Recommendation:** per-sandbox host `certs/` dir bind-mounted rw; no pre-generated host CA is needed because the mount makes mitmproxy's own generated CA both persistent and host-readable. Pass `<certs>/mitmproxy-ca-cert.pem` to the agent's `--tls-upstream-ca-cert`. Because the CA is fixed at agent `create` time, a sidecar must be recreated on the same certs dir (never with a fresh one) or the agent needs `msb modify`/recreate; deleting the certs dir invalidates every agent chained to it.

## Item 4: Chain Evidence

Agent (permissive default egress): `msb create sandbox-mio/base:6 --name spike-netpol-side-agent1 --workdir /root --tls-intercept --proxy socks5://127.0.0.1:19081 --socks5-username u --socks5-password-env SP_PW --tls-upstream-ca-cert <certs>/mitmproxy-ca-cert.pem` with `SP_PW` exported only to the `msb create` process. Sidecar: published with `-p 127.0.0.1:19081:1080`.

```
{"method": "GET", "host": "httpbin.org", "path": "/get", "status": 200, ..., "id": 1}
{"method": "GET", "host": "github.com", "path": "/jdx/mise/info/refs?service=git-upload-pack", "status": 200, ..., "id": 2}
{"method": "POST", "host": "github.com", "path": "/jdx/mise/git-upload-pack", "status": 200, "reqBytes": 102, "respBytes": 774170, ..., "id": 3}
{"method": "GET", "host": "registry.npmjs.org", "path": "/left-pad", "status": 200, ..., "id": 4}
```

The sidecar console shows IP destinations (`GET https://3.137.48.32/get?vol4`, `server connect ...:443`) as in spike 03; the addon's `pretty_host` still logs the name. Deny-egress agent: allowed `httpbin.org` produced a row, `example.com` failed DNS before reaching the proxy.

**TLS interception is required for the viewer in permissive mode.** Without `--tls-intercept`, microsandbox forwards raw TLS bytes through the SOCKS tunnel. mitmproxy then terminates TLS itself with its own CA, which the guest does not trust, so HTTPS fails (`curl (60)`) and no HTTPS row is written; plain HTTP on port 80 does produce full rows. The only way to see HTTPS paths without `--tls-intercept` is to install the mitmproxy CA into the guest, which defeats the double-interception design (the guest would trust the CA that holds every flow). Keep `--tls-intercept` on whenever the viewer is on.

## Item 5: Exposure of the Published Port

`netstat -ano` showed only `TCP 127.0.0.1:19081 0.0.0.0:0 LISTENING` (the `msb` host process), plus short-lived `127.0.0.1` client connections; nothing on `0.0.0.0` or a LAN address. Reachability from an agent sandbox:

- Default agent (no `--net host`): `host.microsandbox.internal` resolves to the gateway (`172.16.0.9`) but connecting to `:19081` fails. The default `allow@public` policy does not include the host.
- Agent created with `--net public,host`: can reach the published port, but the listener demands SOCKS5 credentials (`No authentication method was acceptable` without, success with `u:p`). This is a bypass of the chain only if the agent also knows the password; the password lives in the host environment of `msb create`, not in the guest. If the app ever grants `host` egress to an agent, the proxy password must not be derivable from anything in the guest.
- The sidecar itself (default policy) could not reach `host.microsandbox.internal:<port>` of another pair (`Connection refused`).

Concern level: low with the default policy and authentication; note it in the decision for any future "allow host" option.

## Item 6: Lifecycle

- Sidecar stopped: agent requests fail with `(7) Failed to connect ... after 8056 ms` (the dial times out; fail closed, no bypass).
- Sidecar `start`ed again: chain resumed without touching the agent sandbox (requests 200, rows logged). With the `--init` pattern no re-exec is needed; with the `exec -d` pattern the job must be re-launched.
- Recreate (`msb rm` then `msb create` on the same mounts and port): resumed with the same CA, no agent change.
- `msb stop` hang with `--init`: use `msb stop -f` (see Item 2).
- Sidecar egress: `--net-default-egress deny` breaks the chain because mitmproxy cannot resolve or dial. Leave the default (`allow@public`). `public` excludes private ranges, so the sidecar cannot reach the LAN or host by default.
- The `msb ls` row says `running` as soon as the VM is up, before mitmdump listens; the app's TCP-connect-plus-auth health check from spike 03 still applies, with a start-up budget of about 5 s.

## Item 7: Two Pairs

Pair 1: sidecar a on `127.0.0.1:19081` (password `p`) with agent1; pair 2: sidecar b on `127.0.0.1:19082` (password `p2`) with agent5; each with its own certs, addons copy and flows dir. A request through pair 2 produced one row in pair 2's `flows.ndjson` and zero in pair 1's. Both VMs ran together with the extra agents (up to six sandboxes) without issue. The app must allocate distinct host ports (collision on a taken port was not tested).

## `create_sidecar` Mapping

`MsbProvider::create_sidecar(&SidecarSpec)` and `pull_image(&str)` are currently `CommandFailed("... does not support network-proxy sidecars")` (`src-tauri/src/provider/msb.rs`).

| `SidecarSpec` field / need | msb flag | Status |
|---|---|---|
| `name` | `--name <name>` | maps |
| `image` | positional `IMAGE`; `pull_image` = `msb pull <ref>`; `--pull never` optional | maps; use the fully qualified ref |
| `mounts[].host_path/dest/read_only` | `--mount-dir HOST:DEST[:ro]` (dirs), `--mount-file HOST:DEST` (files) | maps; Windows host paths via `cygpath -m` equivalent (forward slashes); verified `:ro` |
| `command: Vec<String>` | none on `create`. Emulate: `--script start=<shell-joined command>` plus `--init /.msb/scripts/start` | needs a join/quoting layer; commands with `&`, quotes or `$` need escaping per `--script` escape rules |
| `labels` | `--label KEY=VALUE` | flag exists; not exercised |
| `network: InstanceNetwork { name, ip }` | no equivalent (no user networks, no fixed IPs) | does not map; ignore for msb |
| (missing) published port | `-p 127.0.0.1:<host>:<guest>` | new field needed, e.g. `publish: Vec<PortPublish>` (bind addr, host port, guest port); Podman can map it to `-p` too |
| (missing) egress policy | none needed (default `allow@public`) | no field required; must NOT pass a deny default |
| (missing) stop semantics | `msb stop -f` | `stop`/`remove` for sidecars must force when `--init` is used |
| (missing) readiness | none from msb (running before listening) | keep `wait_for_ca` plus a SOCKS connect/auth probe |
| env (not in spec) | `-e KEY=VALUE` | not needed; password lives in the script/args, see below |

SOCKS password: the sidecar script body carries `--proxyauth user:pass`. The password is therefore stored in the sidecar's msb config (`msb inspect` shows scripts); a mitmproxy `--proxyauth` can also be given as `@/path/to/htpasswd-style file`, which was not tried. Treat the sidecar password as local-loopback-only secret as in spike 03.

`create_network` / `remove_network` can stay unsupported for microsandbox: the chain needs no user-defined network, no fixed IP and no shared bridge; the agent reaches the sidecar over host loopback. `proxy_env` (`HTTP_PROXY` pointing at the sidecar IP on the Podman network) is replaced by the agent-side `--proxy`, `--socks5-*` and `--tls-upstream-ca-cert` flags in `create_instance`.

## Recommendations

1. Run mitmproxy as a per-sandbox microsandbox VM from the same image tag as the Podman path; `msb pull` it once (8 s, 98 MiB).
2. Create it with `--mount-dir` for `/addons` (`:ro`), a flows dir (rw) and the confdir (rw), `-p 127.0.0.1:<host>:1080`, default egress, `--script start=...` plus `--init /.msb/scripts/start`; stop it with `msb stop -f`.
3. Keep the per-sandbox certs dir on the host and reuse it across recreates; read the CA from that dir.
4. Agent flags: `--tls-intercept` (required for the viewer), `--proxy socks5://127.0.0.1:<host>`, `--socks5-username`, `--socks5-password-env`, `--tls-upstream-ca-cert`; works with permissive and deny-egress policies.
5. Add a published-port field to `SidecarSpec` and a shell-script launch path for `command`; leave `create_network`/`remove_network` unsupported.
6. Never use `--copy-dir` for addons; do not grant `--net host` to chained agents without reviewing the proxy password exposure.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `msb create IMAGE ... -- mitmdump ...` fails with `unexpected argument` | `create` takes no command; `--entrypoint` takes only an executable and is not auto-run | Register a `--script` and hand PID 1 to it with `--init /.msb/scripts/<name>` |
| Sidecar is `running` and the port is bound, but connections get nothing | The VM started but nothing launched mitmdump (no `--init`, job lost after stop/start) | Use `--init`, or re-run `msb exec -d` after each start; probe the SOCKS port before use |
| `msb stop` on an `--init` sidecar hangs in `draining` forever | mitmdump as PID 1 child does not complete the graceful shutdown, even with a `trap` wrapper | `msb stop -f` (kills in about 0.1 s) |
| Addon never fires, no error, no `flows.ndjson` | Addon delivered with `--copy-dir` (files carry mtime 1970) | Deliver addons with `--mount-dir ...:ro` |
| HTTPS from the chained agent fails with `curl (60)` and no rows | Agent created without `--tls-intercept`; mitmproxy's CA is not in the guest | Always pair `--proxy` with `--tls-intercept` when the viewer is on |
| Chain fails after the sidecar is recreated | Recreated against a fresh certs dir, so the CA changed and the agent's `--tls-upstream-ca-cert` no longer matches | Reuse the same certs dir; deleting it requires recreating the agent |
| Chain dead with `Failed to connect ... after 8056 ms` | Sidecar stopped or its egress denied (`--net-default-egress deny` on the sidecar) | Start the sidecar; keep default egress on it |
| Image not found on `msb create` | Reference not fully qualified | Use `docker.io/mitmproxy/mitmproxy:11.1.3` exactly as pulled |
| Agent can reach the sidecar SOCKS port from the guest | Agent was given `--net host` (or `public,host`) | Do not grant host egress to chained agents; the SOCKS password still gates it |
| `flows.ndjson` has IP hosts in the sidecar console only | microsandbox resolves names and SOCKS-connects by IP | Rely on `request.pretty_host` as the addon does |
| Mount source with a Windows path fails | MSYS rewrites `/c/...` arguments | `MSYS_NO_PATHCONV=1` plus `cygpath -m` for every host path |
