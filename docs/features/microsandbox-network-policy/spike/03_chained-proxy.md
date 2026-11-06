---
type: spike
title: "Spike 03: Chained mitmproxy (`--proxy`) on msb 0.7.12"
feature: microsandbox-network-policy
---
# Spike 03: Chained mitmproxy (`--proxy`) on msb 0.7.12

Hands-on spike run 2026-11-01 on the Windows 11 development host against `msb 0.7.12`, image `sandbox-mio/base:6`. It clears Validation item 3 of `docs/features/microsandbox-network-policy/01_ideas.md` (SOCKS5 listener with authentication, `flows.ndjson` under the proposed TLS layering) and records the evidence for the Open Question "Mitmproxy host placement". All commands ran from Git Bash with `MSYS_NO_PATHCONV=1`; paths passed to `msb` and `mitmdump` went through `cygpath -m`. Podman was down. Every sandbox (`spike-netpol-proxy-a` to `-f`) was removed, no volume was created, and every `mitmdump` process was stopped at the end. Temp files live under `C:\Users\krist\.claude\jobs\c32fd5a9\tmp`.

## Setup

**mitmproxy install.** `uv tool install mitmproxy` with `UV_TOOL_DIR` and `UV_TOOL_BIN_DIR` pointed at fresh directories under the temp dir produced Windows-native `mitmdump.exe` (mitmproxy 12.3.1, CPython 3.12.13, OpenSSL 4.0.3) in one step, with no Podman and no admin rights. The official standalone Windows binary was not needed and not tried. Start-up to "SOCKS v5 proxy listening" took under 6 seconds, and a fresh `--set confdir=<dir>` produced `mitmproxy-ca-cert.pem` on first start.

**Listener** (one per sandbox, each with its own `confdir` and flow log):

```
mitmdump.exe --mode socks5@127.0.0.1:18081 --proxyauth spikeuser:spikepass \
  --set confdir=<tmp>/conf -s <tmp>/flowlog_addon.py
```

`--mode socks5@127.0.0.1:<port>` pins the bind address; the host part is what keeps the listener loopback-only. `--proxyauth user:pass` enables SOCKS5 username/password authentication.

**Addon.** `src-tauri/resources/mitm_flowlog_addon.py` was copied to the temp dir with only the two path constants rewritten (`/addons/rules.json` and `/addons/flows.ndjson` became absolute `C:/...` paths, `rules.json` containing `[]`). No other change; the repo file is untouched. The addon runs unmodified on Windows-native mitmproxy 12.3.1.

**Sandbox.**

```
SP_PW=spikepass msb create sandbox-mio/base:6 --name spike-netpol-proxy-a --workdir /root \
  --net-default-egress deny --net-rule "allow@httpbin.org:tcp:443" --net-rule "allow@registry.npmjs.org:tcp:443" \
  --net-rule "allow@github.com:tcp:443" --net-rule "allow@postman-echo.com:tcp:443" \
  --net-strict --tls-intercept --proxy socks5://127.0.0.1:18081 \
  --socks5-username spikeuser --socks5-password-env SP_PW \
  --tls-upstream-ca-cert "C:/Users/krist/.claude/jobs/c32fd5a9/tmp/conf/mitmproxy-ca-cert.pem"
```

`create` printed nothing and the sandbox was `running`. `runtime.log` records `loaded upstream CA certificates path=... count=1`. No `allow@dns` rule was added; hostname resolution still worked for allowed names (see Item 5).

## Result Table

| # | Check | Result | Evidence |
|---|---|---|---|
| 1 | Authenticated `127.0.0.1` SOCKS5 mitmdump with the project flow-log addon starts and writes `flows.ndjson` | PASS | listener up, addon loaded unmodified apart from paths |
| 2a | Traffic from tools that ignore `HTTP_PROXY` reaches mitmproxy | PASS | `curl --noproxy '*'`, `git ls-remote`, raw `openssl s_client` all produced mitmproxy flows |
| 2b | `flows.ndjson` has method, path, status for HTTPS (double interception) | PASS | rows below; `host` is the real hostname even though SOCKS dials an IP |
| 2c | `127.0.0.1` in `--proxy` is the Windows host, dialled by microsandbox | PASS | guest `curl http://127.0.0.1:18081` fails `Couldn't connect`, while the same listener received the guest's HTTPS flows |
| 3a | Wrong SOCKS password | PASS (fail closed) | guest: `curl: (56) Recv failure: Connection reset by peer` in 0.3 s; no flow row, no `WARN` in `runtime.log`, nothing in mitmdump's console |
| 3b | mitmdump down | PASS (fail closed) | guest: same `(56)` reset, 2.2 s; no bypass. Restarting mitmdump restored the running sandbox without a restart |
| 3c | No `--tls-upstream-ca-cert` | PASS (fail closed, silent) | guest: `curl: (52) Empty reply from server`; no flow row, no `WARN` in `runtime.log` |
| 3d | Denied host stays denied before the proxy | PASS | `https://1.1.1.1` returned the JSON 403; `example.com` failed `Could not resolve host`; flow count and mitmdump console unchanged |
| 4 | `--secret` through the chain | PASS with exposure | httpbin echoed `Authorization: Bearer <real value>`; mitmproxy saw the real value (below) |
| 5a | `--proxy http://` CONNECT mode (`mitmdump --mode regular@127.0.0.1:18083 --proxyauth`) | PASS | same flows (`GET httpbin.org/get` 200, `git ls-remote` to github.com) with `--http-connect-username` and `--http-connect-password-env` |
| 5b | DNS and non-DNS UDP | PARTIAL | DNS never reaches the proxy; non-DNS UDP not conclusively tested (below) |
| 6 | Two sandboxes, two proxy ports, concurrently | PASS | sandbox a on 18081 and sandbox b on 18082; each request landed only in its own `flows.ndjson` (A+1, B+1) |

## Item 2: Double Interception Evidence

`msb exec spike-netpol-proxy-a -- ...`:

```
curl -sS --noproxy '*' -i https://httpbin.org/get                        -> HTTP/1.1 200 OK
curl -sS --noproxy '*' https://registry.npmjs.org/left-pad                -> 200
git ls-remote https://github.com/jdx/mise HEAD                            -> c584fcce... HEAD
printf 'GET /anything/via-openssl ...' | openssl s_client -quiet -connect httpbin.org:443 -servername httpbin.org
```

The guest still sees microsandbox's CA as the issuer (`issuer=CN = microsandbox CA, O = microsandbox`, `Verification: OK`), so the guest trusts only microsandbox's CA. `flows.ndjson` rows (ids 1 to 5; `rules.json` was `[]`):

```
{"method": "GET", "host": "httpbin.org", "path": "/get", "status": 200, "durationMs": 225.05, "reqBytes": 0, "respBytes": 200, "secretInjected": false, "id": 1}
{"method": "GET", "host": "registry.npmjs.org", "path": "/left-pad", "status": 200, "respBytes": 22573, ...}
{"method": "GET", "host": "github.com", "path": "/jdx/mise/info/refs?service=git-upload-pack", "status": 200, ...}
{"method": "POST", "host": "github.com", "path": "/jdx/mise/git-upload-pack", "status": 200, "reqBytes": 102, "respBytes": 773125, ...}
{"method": "GET", "host": "httpbin.org", "path": "/anything/via-openssl", "status": 200, ...}
```

mitmproxy's own console shows the destination as an IP (`GET https://16.58.42.220/get`, `server connect 16.58.42.220:443`) because microsandbox resolves the name itself and SOCKS-connects by IP. The addon's `request.pretty_host` still yields `httpbin.org` (from the intercepted Host header / SNI), so the traffic viewer shows names. Anything keyed on `flow.server_conn.address` would see the IP.

A node `fetch` check was not run: the image has no node preinstalled and `nodejs.org` is not on the spike allowlist. The curl, git and raw-TLS tools cover the "ignores `HTTP_PROXY`" requirement.

## Item 3: Negative Checks

| Case | Guest sees | Host-side signal | Fails closed? |
|---|---|---|---|
| Wrong SOCKS5 password (`SP_PW=WRONG`) | `curl: (56) Recv failure: Connection reset by peer`, fast (0.3 s) | none visible: no `WARN`/`ERROR` in `runtime.log`, nothing in mitmdump's console, no flow row | yes |
| mitmdump not running | same `(56)` reset, 2.2 s; a request carrying the secret placeholder fails identically | none | yes; no direct fallback |
| mitmdump restarted | next request `200`, no sandbox restart | listener is redialled per connection | n/a |
| No `--tls-upstream-ca-cert` | `curl: (52) Empty reply from server` (also with `-k`, because the failure is on microsandbox's upstream leg) | none; mitmdump logged no client connect at default verbosity, so the cause is not visible proxy-side either | yes |
| Denied host | JSON 403 `network_policy_denied` (raw IP) or DNS failure (name); flow count unchanged | nothing reaches mitmproxy | yes |

No proxy failure mode produced a direct (unproxied, unrecorded) connection. The three proxy-chain failures are indistinguishable from each other to the guest, and `runtime.log` is silent, so the app has to health-check the listener (TCP connect plus authentication) itself before showing "proxy unreachable".

## Item 4: Secret Through the Chain

Sandbox `spike-netpol-proxy-b` added `--secret "SPIKE_TOK:headers=[authorization]@httpbin.org"` (value supplied only in the environment of the spawned `msb create`) to the same flags, with its own listener on 18082.

- `curl -H "Authorization: Bearer $SPIKE_TOK" https://httpbin.org/headers` (the guest variable holds the placeholder) returned `"Authorization": "Bearer spike-secret-12516275"`: substituted.
- The same request to `postman-echo.com` returned the secret-policy 403 (`secret_policy_denied`), as in spike 02.
- To observe what mitmproxy sees, a second diagnostic addon (`seen_addon.py`, test only, not a project file) wrote `request.headers["authorization"]`. It recorded `httpbin.org authorization=Bearer spike-secret-12516275`: mitmproxy sees the **real value**, confirming substitution happens before the upstream leg.
- The project flow-log addon wrote no header values. A grep for the value over the flow log, `rules.json` and both mitmdump consoles (default verbosity) returned 0 matches. The exposure therefore exists only to code running inside mitmproxy (addons, `--set flow_detail=3` or higher, a saved flow file, `mitmweb`).
- The flow row for that request has `"secretInjected": false`. That field is computed by matching `rules.json` host/path rules (the `mitm_addon.py` injector), not by observing microsandbox's substitution, so on microsandbox sandboxes it is always false and misleading.

Exposure to flag: the real credential is in plaintext in mitmproxy's memory on the Windows host, in a process that also holds the per-sandbox CA key. This is the Open Question 5 trade-off (traffic viewing as an opt-in separate from policy) and is bounded by SOCKS5 authentication on a loopback listener.

## Item 5: CONNECT Mode, DNS and UDP

**HTTP CONNECT.** `mitmdump --mode regular@127.0.0.1:18083 --proxyauth spikeuser:pass3` with `--proxy http://127.0.0.1:18083 --http-connect-username spikeuser --http-connect-password-env SP_PW` worked on the first try: `curl https://httpbin.org/get` returned 200 and `git ls-remote` succeeded, and `flows.ndjson` carried method, path and status. Differences from SOCKS5: only the flag family changes (`--http-connect-*` instead of `--socks5-*`), and mitmproxy's mode is `regular`. No functional advantage was observed, so SOCKS5 stays the recommendation (it carries any TCP, not only tunnels).

**DNS.** Guest `getent hosts httpbin.org` resolved with no `allow@dns` rule, via the gateway (`/etc/resolv.conf`: `nameserver 172.16.0.5`). A DNS query to `8.8.8.8:53` over TCP (allowed in sandbox f) returned an NXDOMAIN-class answer (flags `81 83`) for `example.com`, so port 53 is answered by microsandbox's gateway even toward other servers. Neither form produced any mitmproxy console line. DNS does not traverse the proxy.

**Non-DNS UDP.** The guest has no `nc`, `python3` or `perl`. A `/dev/udp/8.8.8.8/53` write with an `allow@8.8.8.8:udp:53` rule returned no data and produced no mitmproxy line. mitmproxy's SOCKS5 mode does not implement UDP ASSOCIATE, so UDP cannot be proxied through it. Whether non-DNS UDP is dropped or sent directly by microsandbox (a bypass of recording) was not isolated; treat UDP as outside the traffic viewer.

Raw non-TLS TCP to a non-443 port was only exercised against `8.8.8.8:53`, which the gateway intercepts, so a generic "non-HTTP TCP goes through SOCKS" check was not run.

## Item 6: Port per Sandbox

Two mitmdump listeners (18081 with `conf`, 18082 with `conf2`, each with its own addon copy and `flows.ndjson`) served two sandboxes simultaneously. One `curl` per sandbox added exactly one row to its own flow log. Distinct passwords per listener worked (`pass2` for 18082). A third regular-mode listener (18083) ran alongside. Each needs its own `confdir`, so the CA is per sandbox, matching the design.

## Differences from `01_ideas.md`

| `01_ideas.md` says | 0.7.12 reality |
|---|---|
| Chained SOCKS5 with `--tls-upstream-ca-cert` is "Proposed, pending Validation" | Confirmed end to end: microsandbox intercepts the guest leg, mitmproxy intercepts the upstream leg, `flows.ndjson` has method, path and status for HTTPS. The guest trusts only microsandbox's CA |
| A Windows-native `mitmdump` bundled through `build.rs` is one candidate | Works: `uv tool install mitmproxy` yields a working 12.3.1 `mitmdump.exe` with no Podman; the unmodified flow-log addon runs on it |
| "`mitm_flowlog_addon.py` records no header values ... the value never enters the guest" | Confirmed (0 matches). But `secretInjected` is wrong for microsandbox sandboxes: it derives from `rules.json`, not from microsandbox's `--secret` substitution |
| SOCKS5 authentication is "mandatory" | Enforced: a wrong password yields a reset, a closed listener yields a reset, neither bypasses |
| (not covered) | Proxy failures are silent on the host side (no `runtime.log` line) and look identical to the guest; the app must health-check the listener |
| (not covered) | The proxy sees IP destinations; hostnames in `flows.ndjson` come from `pretty_host` |
| (not covered) | DNS never traverses the proxy; SOCKS5 mitmproxy cannot carry UDP |

## What Should Change in `01_ideas.md`

- Validation item 3: mark cleared. Add that the listener must use `--mode socks5@127.0.0.1:<port>` (host part required) and a per-sandbox `confdir`.
- TLS layering section: state "confirmed", and add that a missing or wrong `--tls-upstream-ca-cert` fails closed with no host-side log.
- Open Question "Mitmproxy host placement": adopt the recommendation below.
- Secrets exposure paragraph: add that `secretInjected` in `flows.ndjson` needs a different source for microsandbox (for example, the app marks rows for hosts that carry a `--secret` binding), and must stop depending on `rules.json`.
- Open Question 5: the chained proxy is fail-closed and authenticated, so the remaining reason for an opt-in is the in-flight secret exposure and the extra process.
- Add a proxy health check requirement (TCP connect plus SOCKS authentication), because failures are indistinguishable to the guest and silent in the host log.
- Note that the viewer covers HTTPS over TCP only; DNS and UDP are outside it.

## Recommendation: Mitmproxy Host Placement

Use a Windows-native `mitmdump`, one process per traffic-viewed microsandbox sandbox, bound to `127.0.0.1:<app-allocated port>`:

- It worked with no container runtime, so a microsandbox sandbox does not depend on a running Podman machine. The Podman sidecar with a `127.0.0.1` publish was not tested (Podman was down) and would couple microsandbox to Podman.
- The install method that worked is `uv tool install mitmproxy` (12.3.1). For shipping, bundle either the official standalone Windows `mitmdump` build or a pinned `uv`-managed environment through `build.rs`. The standalone binary was not evaluated here and is the first thing to confirm (size, antivirus behaviour, start time).
- A second microsandbox sandbox running mitmproxy adds a VM and a published port with no benefit seen here, and was not tested.
- Lifecycle to build: the app owns the process (start before the sandbox, stop after it), the flow log directory, a per-sandbox `confdir`, a random SOCKS password passed through `--socks5-password-env`, and the listener health check.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| Guest sees `curl: (56) Recv failure: Connection reset by peer` on every HTTPS request | The chained proxy is down or the SOCKS5 password is wrong; microsandbox fails closed and logs nothing host-side | Health-check the listener and credentials before starting the sandbox; do not rely on `runtime.log` |
| Guest sees `curl: (52) Empty reply from server` | `--tls-upstream-ca-cert` is missing or does not match mitmproxy's CA, so the upstream leg fails (`-k` does not help) | Pass the listener's `confdir/mitmproxy-ca-cert.pem` to `msb create --tls-upstream-ca-cert` |
| The SOCKS listener is reachable beyond loopback | The bind address was not given | Use `--mode socks5@127.0.0.1:<port>` and always `--proxyauth` |
| Flow rows or the mitmdump console show IP addresses instead of names | microsandbox resolves the name and SOCKS-connects by IP | Key on `request.pretty_host` (the shipped addon does), not `server_conn.address` |
| `secretInjected` is always false for microsandbox sandboxes | The field is derived from `rules.json` (the `mitm_addon.py` injector), not from microsandbox substitution | Derive it from the sandbox's `--secret` host bindings |
| mitmproxy observes the real secret | microsandbox substitutes before the upstream leg | Keep header-logging addons off, avoid `flow_detail` above default and flow-file saving; document the exposure |
| Two sandboxes share one CA or one flow log | Same `confdir` or addon paths on both listeners | One `confdir`, one flow directory and one port per sandbox |
| Traffic viewer shows nothing for DNS or UDP | DNS is answered by the microsandbox gateway and SOCKS5 mitmproxy has no UDP | Treat the viewer as TCP/HTTPS only |
| `uv tool install` puts executables outside `PATH` | `UV_TOOL_BIN_DIR` was a custom directory | Call `<bin dir>/mitmdump.exe` by absolute path |
| Stopping "mitmdump" by command-line match killed the shell | A `-like '*<port>*'` filter also matched `bash.exe` processes carrying the port in their own command line | Filter on `Name -eq 'mitmdump.exe'` plus its `python.exe` children |
