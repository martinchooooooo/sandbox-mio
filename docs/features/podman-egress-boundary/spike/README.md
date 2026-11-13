---
type: spike
title: "Spike Artefacts — Podman Egress Boundary Phase A"
feature: podman-egress-boundary
---
# Spike Artefacts — Podman Egress Boundary Phase A

Hands-on run of the seven Phase A gating spikes of [../02_dev.md](../02_dev.md), on 2026-11-13, against `podman-machine-default` (Podman 5.8.6, rootless, netavark, WSL kernel 6.6.87.2, default log driver `journald`) with `docker.io/ironsh/iron-proxy:0.56.0` and `localhost/sandbox-mio/base:7`. Scripts ran from Git Bash with `MSYS_NO_PATHCONV=1`. Every resource was named `spike-peb-*` and removed by `teardown.sh`; the default `podman` network and unrelated containers were not touched. The pulled iron-proxy image was kept. iron-proxy source was read at tag `v0.56.0` (commit `3d96ce8`), not the default branch. The summary and the decisions each result affects live in [../02_dev.md](../02_dev.md), Spike Results; this document holds the evidence.

## Scripts

| File | Purpose |
|---|---|
| `lib.sh` | Shared names, the `podman --connection podman-machine-default` prefix, and the `mgmt` helper that calls the management API with busybox `wget` and a bearer token |
| `gen_ca/` | Standalone Rust crate: an `rcgen` 0.13 ECDSA P-256 CA with ten-year validity, written as `ca.pem` and `ca-key.pem` (PKCS#8) — Resolved Decision 11 |
| `render_config.sh` | Writes `config/config.yaml` with a JSON body (Resolved Decision 12): listeners, the Resolved Decision 17 deny list, allowlist, secrets, loopback metrics and management |
| `setup.sh` | Builds the Concrete Model topology: `--internal --disable-dns` network, dual-homed sidecar with `k8s-file` logging, sandbox with `--dns`, `proxy_env` variables and the `inject_ca` transport |
| `exec_transport.sh` | SSH over the `podman exec … mio-sshd` ProxyCommand, with a `-L` forward (port forwards) and a `-R` forward (local-LLM reverse forward) |
| `capture_fixtures.sh` | Exercises every audit-row shape once and saves raw `podman logs --timestamps` output |
| `fixtures/podman-logs-timestamps.txt` | 28 captured lines: allowed, warn, CONNECT, tunnel inner request, two reject shapes, header swap, query swap, inject, secret unavailable, upstream-deny error, SSH-over-CONNECT, reload success, reload rejection |
| `fixtures/config.yaml` | The JSON-bodied config the sidecar last ran with, including one `replace` and one `inject` secrets entry; the key-name source for Phase C's renderer tests |
| `teardown.sh` | Removes every `spike-peb-*` container and network |

Order: build `gen_ca` into `$SPIKE_STATE/certs`, `render_config.sh`, `setup.sh`, then the checks. `SPIKE_STATE` is the stand-in for `<app_data_root>/proxy/<sandbox>/`.

## Result Table

| # | Check | Result | Evidence |
|---|---|---|---|
| 1a | Internal network plus dual-homed sidecar | PASS | Sandbox route table holds only `10.90.250.0/24`; sidecar has a default route via the `podman` network (`10.88.0.1`) |
| 1b | Raw sockets from the sandbox | PASS (blocked) | `1.1.1.1:443`, `1.1.1.1:80`, `140.82.112.3:22`, `10.88.0.1`, WSL DNS `10.255.255.254`: "Network is unreachable" |
| 1c | Host exposure through the internal bridge | PASS (none) | The `--internal` network has no gateway address; the `podman1` bridge carries no address in the rootless netns, so the machine's `0.0.0.0` SSH listeners are unreachable |
| 1d | Exec transport and forwards | PASS | SSH command over the `mio-sshd` ProxyCommand; `-L` reached an in-sandbox loopback service; `-R` reached a host server from sandbox loopback with `proxy_env` set |
| 1e | Default `podman` network as the external leg | PASS | Present on the rootless machine with `dns_enabled: false`; no `aardvark-dns` needed; Resolved Decision 3 fallback not triggered |
| 2a | iron-proxy DNS | PASS | Every name, including invented exfiltration names, resolves to the sidecar; AAAA yields nothing (glibc maps the A answer) |
| 2b | Non-proxy-aware clients | PASS | `curl --noproxy '*'` over 443 and 80, Node `fetch` and Python `urllib` without proxy variables all reach allowed hosts through the DNS route |
| 2c | Resolver override and odd ports | PASS (blocked) | `nameserver 8.8.8.8` fails; a DNS-routed connection to port 8443 is refused |
| 3a | Audit shape | PASS | One JSON object per line on stderr; fields nested under `audit`; see Audit Record Shape |
| 3b | `podman logs --since` | PASS | Nanosecond timestamps; `--since <T>` is inclusive of the line stamped `T`. Phase E later found that the Podman 5.8.2 client truncates `--since` to the whole second, so earlier lines of that second come back too (`../02_dev.md`, Phase E) |
| 3c | `k8s-file` with `max-size=1mb` | PARTIAL | conmon truncates `ctr.log` to zero at the cap, no rotation file: 3,500 requests left 1,075 lines and lost every earlier line, startup lines included |
| 3d | Logs of a stopped sidecar | PASS | Readable while stopped; a restart appends to the same log |
| 4 | Toolchains through the tunnel with the rcgen CA | PASS | `git clone` and `git ls-remote` over HTTPS, `mise install node@22` and `python@3.12`, `npm install`, `pip install` |
| 5a | JSON-bodied `config.yaml` | PASS | Loads at boot and on reload |
| 5b | `POST /v1/reload` through `podman exec` | PASS with change | busybox `wget --post-data=` works only with `Authorization: Bearer`; without the token: 401; `management.listen` requires the variable named by `management.api_key_env` (default `IRON_MANAGEMENT_API_KEY`) at boot |
| 5c | Secret rotation | PASS | A rewritten `file` source is used only after a reload or restart |
| 5d | Invalid config | PARTIAL | Unknown top-level key: 422, old pipeline kept, reason in the sidecar log only (busybox `wget` hides the body). Unknown key inside a transform's `config`: silently accepted |
| 5e | Missing secret file | PASS | Reload succeeds; the request passes without the secret; audit annotates `secret_unavailable`; the failure is cached for one minute |
| 5f | Open stream across a reload | PASS | An 8-second chunked response completed with all bytes while a reload ran mid-stream |
| 6a | `secrets` config keys | PASS | `replace` block (`proxy_value`, `match_headers`, `match_query`, `match_path`, `match_body`, `require`) and an `inject` block (`header` or `query_param`, `formatter`, `require`); legacy top-level `proxy_value` still accepted; `match_query` defaults to false |
| 6b | `inject` mode | PASS | The sidecar set `X-Mio-Injected` with the real value on a request that carried no credential, only on the rule's path |
| 6c | Wildcard apex | PASS | `*.example.com` matches `example.com` |
| 6d | Private-address deny | PASS | Allowed names resolving to `10.88.0.1` and `::1`: 502, audit `error` names the CIDR |
| 6e | Ports through the tunnel listener | PASS (any port) | CONNECT to `portquiz.net:8080` (HTTP) and `tls-v1-2.badssl.com:1012` (TLS) allowed; SSH through CONNECT to `github.com:22` closed after the CONNECT was logged as `allow` |
| 6f | Literal-IP HTTPS | FAIL (edge) | CONNECT to `1.1.1.1:443` allowed, then no request row: the client sends no SNI for an IP, so no leaf certificate is minted |
| 7a | Tag format | PASS | Docker Hub tags are bare semver (`0.56.0`, `latest`, `-rc.N` pre-releases); `0.56.0` does not parse as an integer |
| 7b | Ports 53/80/443 in a rootless container | PASS | The image runs as uid 0 inside the user namespace; every listener bound |
| 7c | Footprint | PASS | Image 64 MB (Alpine 3.24, busybox `wget`, `openssl`, no `curl`); RSS 37 MB idle, peak 39 MB while streaming a 100 MB download |

## Audit Record Shape

Read from `fixtures/podman-logs-timestamps.txt`; the README example (flat fields) is wrong for 0.56.0.

- Every line carries `time` (UTC), `level` and `msg`. Only `msg: "request"` lines are traffic; reload, secret-fetch and short-circuit messages share the stream.
- `audit` holds `host` (with `:port` for CONNECT and non-default ports), `method`, `path`, `remote_addr` (the sandbox address), `sni`, `mode`, `action` (`allow`, `reject`, `error`), `status_code` and `duration_ms`. No byte counts exist.
- `rejected_by` (transform name) and `error` (string) sit at the top level, beside `audit`. Rejects log at `WARN`, errors at `ERROR`.
- `request_transforms` is an array of `{name, action, duration_ms, annotations?}`. The allowlist's `open`-mode marker is `annotations.action = "warn"`. The secrets transform annotates `swapped` or `injected` (each a list of `{secret, locations}`, `secret` being the source's file path) and `secret_unavailable`.
- A tunnelled request produces two rows: the CONNECT (path empty) and the inner request carrying a `tunnel` object with `target`. A rejected CONNECT produces only the CONNECT row.
- No secret value appeared anywhere in the log during the run.

## Other Observations

- The sandbox receives `.1` on the internal network, because an `--internal` network has no gateway; the sidecar pinned at `.2` must exist first, as today.
- With the sidecar stopped, sandbox connections and DNS lookups time out instead of failing fast, since the sidecar's address no longer answers.
- `metrics.listen` cannot be disabled: an empty value defaults to `:9090` on every interface (`internal/config/config.go` at the tag), so the renderer must bind it to `127.0.0.1`.
- The allowlist's `cidrs` match only hosts written as literal IP addresses, never a hostname resolving into the range (`internal/hostmatch/hostmatch.go` at the tag); not exercised live.
- A `file` secret source returns the file's exact bytes, so the value file must not end in a newline.
- Host-side atomic renames were visible inside the container through both a directory mount and a single-file bind mount.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| A spike script fails with "unable to connect to Podman socket … 127.0.0.1:51105" | The default Podman connection points at a stopped machine | Run through `lib.sh`, which pins `--connection podman-machine-default`, or set `SPIKE_CONNECTION` |
| A file named `NUL` appears in the spike directory | `podman machine ssh` from Git Bash writes a known-hosts line to a literal `NUL` file | Delete the `NUL` file |
| Fixture lines from earlier steps are missing from `podman logs` | The sidecar's `k8s-file` log was truncated at `max-size` by a later flood | Re-run `capture_fixtures.sh` on a fresh topology before any flood test |
