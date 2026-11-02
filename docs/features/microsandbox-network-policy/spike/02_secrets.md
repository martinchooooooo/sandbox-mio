# Spike 02: Host-Held Secrets (`--secret`) on msb 0.7.12

Hands-on spike run 2026-11-01 on the Windows 11 development host against `msb 0.7.12`, image `sandbox-mio/base:6`. It clears Validation item 2 of `docs/features/microsandbox-network-policy/01_ideas.md` and records the secret lifecycle the design depends on (start, restart, `msb modify`, persistence). Item 3 (chained `--proxy`) is left for spike C. All commands ran from Git Bash with `MSYS_NO_PATHCONV=1`. Every sandbox (`spike-netpol-secret-*`) was removed at the end; no volume was created. Dummy value: `spike-secret-<random>` (written only to a temp file under `C:\Users\krist\.claude\jobs\c32fd5a9\tmp`).

## Setup

Echo services: `https://httpbin.org/headers` (authorized host) and `https://postman-echo.com/headers` (second allowed host). Both worked. The value is set on the spawned `msb` command only (`SPIKE_TOK=$V msb create ...`), never exported and never passed through `--env`.

```
SPIKE_TOK=$V msb create sandbox-mio/base:6 --name spike-netpol-secret-a --workdir /root \
  --net-default-egress deny --net-rule "allow@httpbin.org:tcp:443" --net-rule "allow@postman-echo.com:tcp:443" \
  --net-strict --tls-intercept --secret "SPIKE_TOK:headers=[authorization]@httpbin.org"
```

## Result Table

| # | Check | Result | Evidence |
|---|---|---|---|
| 1 | Placeholder in `Authorization` toward the bound host arrives substituted; guest sees only the placeholder | PASS | `msb exec ... env` shows `SPIKE_TOK=$MSB_SPIKE_TOK` (the guest variable keeps the host variable's name and holds the literal placeholder `$MSB_SPIKE_TOK`). `curl -H 'Authorization: Bearer $MSB_SPIKE_TOK' https://httpbin.org/headers` returned `"Authorization": "Bearer spike-secret-204077444"` |
| 2 | Same placeholder toward a different allowed host is blocked, per violation action | PASS | all three actions return the same JSON 403 (below); they differ in logging and sandbox fate |
| 3 | Placeholder in a non-authorized position | PASS (fail-safe) | toward the bound host: passed literally, not substituted, not blocked. Toward any other host: blocked in every position |
| 4a | Value re-read from the starting `msb` process at every start | PASS | `msb start` without the variable fails; with a changed value the new value arrives |
| 4b | `msb modify --secret` / `--secret-rm` live | PARTIAL | rotating the value and changing the host list apply live; adding a secret, or changing options, requires `--restart` or `--next-start`; `--secret-rm` applies live |
| 4c | Value never persisted in plaintext | PASS (one caveat) | `inspect` shows `"value": ""` and `source: {kind: env, var}`; no match in `db`, `run`, or sandbox config. The only matches are `logs/exec.log`, because the echo service returned the real value to guest stdout |
| 5 | Interaction with `--net-strict`, `--tls-intercept`, `--tls-bypass` | PASS with surprises | see Item 5 |

## Item 2: Violation Actions

Request: `curl -H 'Authorization: Bearer $MSB_SPIKE_TOK' https://postman-echo.com/headers` from a sandbox whose secret is bound to `httpbin.org` only.

Guest response (identical for `block` and `block-and-log`, and for header, query and body positions):

```
HTTP/1.1 403 Forbidden
Content-Type: application/json
Connection: close
Cache-Control: no-store
{"code":"secret_policy_denied","domain":"postman-echo.com","message":"Request blocked by secret policy."}
```

| Action | Guest sees | Host log | Sandbox afterwards |
|---|---|---|---|
| unset (default) | JSON 403 | recorded as `action=block-and-log` in `inspect` (`secrets.violation_action`) and logged; the default is `block-and-log` | keeps running; a clean request to the same host succeeds |
| `block` | JSON 403 | no violation line at all (grep for `secret violation` in `runtime.log` returned 0) | keeps running |
| `block-and-log` | JSON 403 | `WARN microsandbox_network::engine::secrets::handler: secret violation: placeholder detected where substitution or passthrough is not permitted action=block-and-log secret_env_var=SPIKE_TOK placeholder=$MSB_SPIKE_TOK protocol=http/1.1 sni=postman-echo.com host=postman-echo.com method=GET path=/headers location=header match_form=raw guest_dst=... ` (`location=` is `header`, `query` or `body`) | keeps running |
| `block-and-terminate` | no response: `error: exec session ended without exit event`, exit 1 | `ERROR ... secret violation: ... - terminating action=block-and-terminate ...` then `WARN microsandbox_runtime::runner::vm: secret violation requested sandbox termination`, `Vmm is stopping` | `msb ls` shows `stopped` immediately |

Log location: `C:\Users\krist\.microsandbox\sandboxes\<name>\logs\runtime.log` (`msb logs <name>` shows guest exec output, not these lines). The log line carries the placeholder, never the value.

Because the sandbox the app creates is typically driven by an agent, `block-and-terminate` turns a prompt-injection attempt into a hard stop; `block-and-log` is the recommended default so the UI can surface `runtime.log` violation lines.

## Item 3: Position Matrix (`headers=[authorization]`, bound to `httpbin.org`)

| Request | Bound host `httpbin.org` | Other allowed host `postman-echo.com` |
|---|---|---|
| `Authorization: Bearer $MSB_SPIKE_TOK` | substituted | blocked (`secret_policy_denied`, `location=header`) |
| `X-Other: $MSB_SPIKE_TOK` (non-listed header) | passed literally (`"X-Other": "$MSB_SPIKE_TOK"`) | blocked |
| `?k=$MSB_SPIKE_TOK` | passed literally | blocked (`location=query`) |
| body `k=$MSB_SPIKE_TOK` | passed literally | blocked (`location=body`) |
| plain HTTP (port 80 allowed) to the bound host | blocked `secret_policy_denied` (`require_tls_identity: true`) | n/a |
| unknown placeholder (`$MSB_OTHER`, no such secret) | echoed literally | echoed literally; no check, only registered placeholders are policed |

So a placeholder in an unauthorized position on the bound host is not substituted and not blocked: the agent only leaks its own placeholder string. The value never leaves in a position the user did not list.

## Item 4: Lifecycle

Sandbox lifecycle (`spike-netpol-secret-block-and-terminate`, stopped after the violation):

| Step | Result |
|---|---|
| `msb start <n>` without `SPIKE_TOK` | `error: invalid config: secret SPIKE_TOK: host environment variable SPIKE_TOK is not set`, exit 1, sandbox stays `stopped` |
| `msb exec <n> -- ...` on a stopped sandbox, no variable | same error: `exec` auto-starts a stopped sandbox and the start needs the variable too |
| `SPIKE_TOK=spike-secret-NEWVALUE-999 msb start <n>` | started; httpbin echoed `Bearer spike-secret-NEWVALUE-999` (the new value, not the create-time value) |
| `msb stop`, then `SPIKE_TOK=spike-secret-THIRD-111 msb exec <n> -- curl ...` | echoed `THIRD-111`; the sandbox ran for the exec and returned to `stopped` afterwards (exec on a stopped sandbox is an ephemeral start) |

So every start, including the implicit one in `exec`, re-reads the variable from that process's environment. A start without it fails closed. In the app, every `msb` invocation that can start a sandbox (`start`, `exec`, `restart`, `modify --restart`) needs the value in its environment.

`msb modify` (running sandbox `spike-netpol-secret-a`):

| Command | Result |
|---|---|
| `OTHER=... msb modify <n> --secret "OTHER:headers=[authorization]@postman-echo.com"` (new secret) | `error: cannot modify "spike-netpol-secret-a" without a restart / secret OTHER requires restart / no changes were applied`, hints `--restart` and `--next-start`. Until applied, `$MSB_OTHER` goes upstream literally |
| `SPIKE_TOK=...ROTATED-6 msb modify <n> --secret "SPIKE_TOK@httpbin.org"` (rotate value, same rule) | `Modified`, live; next request carried `ROTATED-6` |
| `SPIKE_TOK=... msb modify <n> --secret "SPIKE_TOK@httpbin.org,postman-echo.com"` (host list change) | `Modified`, live; and dropping `postman-echo.com` again was live (that host went back to `secret_policy_denied`) |
| `msb modify <n> --secret "SPIKE_TOK"` (no `@HOST`) | `error: secret must be in format ENV[:OPTIONS]@HOST[,HOST...]`; the host list is mandatory even for a rotation |
| `msb modify <n> --secret "SPIKE_TOK@httpbin.org"` with the variable unset | `error: invalid config: secret SPIKE_TOK: host environment variable SPIKE_TOK is not set` |
| `SPIKE_TOK=... msb modify <n> --secret "SPIKE_TOK:query@httpbin.org"` (options change) | `requires restart`, nothing applied; `--restart` applies it now |
| `msb modify <n> --secret-rm SPIKE_TOK` | `Modified`, live, no variable needed. Afterwards the guest still shows `SPIKE_TOK=$MSB_SPIKE_TOK` and the placeholder now reaches httpbin literally (`"Authorization": "Bearer $MSB_SPIKE_TOK"`); `inspect` no longer lists the secret |
| `SPIKE_TOK=...ROTATED-3 msb modify <n> --secret "SPIKE_TOK:headers=[authorization]@httpbin.org,postman-echo.com" --restart` (re-add) | `Modified`, `Restarted`; the new value arrived at both hosts |

Persistence of the value (grep of `C:\Users\krist\.microsandbox` for every dummy value, text and binary, including `db/msb.db*`, `run`, `sandboxes`):

- `msb inspect --format json` stores the binding as `{allowed_hosts, env_var, placeholder, passthrough_hosts, require_tls_identity, source: {kind: "env", var: "SPIKE_TOK"}, substitution: {headers, header_fields, query, body}, value: ""}` plus `violation_action`. The `value` field is empty.
- The only files containing a dummy value are `sandboxes/<name>/logs/exec.log`, which captures guest stdout. The real value appears there only because httpbin echoed it back into the guest's `curl` output. A guest command never sees the real value except through an upstream that reflects it, and that reflection then lands in this host log and in the guest. Echo or debug endpoints on an authorized host defeat the isolation.

## Item 5: Strict, Interception and Bypass

All sandboxes used `--net-default-egress deny`, allow rules for both hosts (plus port 80 for `httpbin.org` where noted) and `--secret "SPIKE_TOK:headers=[authorization]@httpbin.org"`.

| Variant | HTTPS to `httpbin.org` with placeholder | HTTP (port 80) to `httpbin.org` | HTTPS to `postman-echo.com` |
|---|---|---|---|
| `--net-strict` + `--tls-intercept` (item 1) | substituted | not tested (port 80 not allowed) | blocked |
| default strict, no `--tls-intercept` flag | substituted | blocked, `secret_policy_denied` | blocked |
| `--tls-intercept --net-strict=false` | substituted | blocked, `secret_policy_denied` | blocked |
| `--tls-intercept --tls-bypass httpbin.org` (strict) | `curl: (35) SSL_ERROR_SYSCALL` (strict cannot inspect an uninspected connection) | blocked | blocked |
| `--tls-intercept --tls-bypass httpbin.org --net-strict=false` | allowed, placeholder reaches httpbin unsubstituted (`"Authorization": "Bearer $MSB_SPIKE_TOK"`) | not tested | blocked (still intercepted) |

Findings:

- `--secret` turns TLS interception on by itself. A create with no `--tls-intercept` shows `network.tls.enabled: true` in `inspect` when a secret is present, and `false` when none is; the HTTPS call to the bound host worked without the flag, while the same policy without a secret failed with `SSL_ERROR_SYSCALL`. Pass `--tls-intercept` anyway for clarity, but a secret never runs uninspected.
- A bypassed host never receives the real value: bypass plus secret yields the placeholder, not the value. With strict on, bypass makes that host unreachable over HTTPS.
- `--net-strict=false` does not weaken secret checks; the same blocks apply.
- Plain HTTP never receives a substituted value, even to the bound host.

## Differences from `01_ideas.md`

| `01_ideas.md` says | 0.7.12 reality |
|---|---|
| `--secret NAME[:OPTIONS]@HOST[,HOST...]` reads the value from the `msb` process environment at start | Confirmed. The first token is the host variable name and also the guest variable name; the guest holds the literal string `$MSB_<NAME>` in `<NAME>` (not a `MSB_`-prefixed variable). `msb exec` on a stopped sandbox counts as a start and needs the variable too |
| `--secret-violation-action` selects block, block-and-log or block-and-terminate | Confirmed. Default is `block-and-log` (recorded in `inspect`); `block` writes no log line; `block-and-terminate` stops the VM and the in-flight `exec` fails with `exec session ended without exit event` |
| "Secret changes apply live through `msb modify --secret`" (Alternatives, recreate paragraph; Common Gotchas) | Only partly. Value rotation and host-list changes are live. Adding a new secret or changing options (`headers=`, `query`, `body`) needs `--restart` (applies now, restarts the VM) or `--next-start`. `--secret-rm` is live but leaves the guest variable and placeholder behind (the placeholder then travels upstream literally). The rotation must repeat `@HOST[,HOST...]` and have the value in the `modify` process environment |
| Injection requires `--tls-intercept` | `--secret` enables interception implicitly; the flag is optional but harmless |
| Placeholder toward another host is blocked | Confirmed, in every position (header, query, body) toward any host not in `allowed_hosts` and not in `passthrough=`. Toward the bound host in a non-listed position it is passed through literally rather than blocked |
| Secret values never in guest-visible files | Confirmed for config and db. New: `logs/exec.log` records guest stdout, which holds the real value if an authorized upstream reflects it. `msb logs` reads that file |

## What Should Change in `01_ideas.md`

- Validation item 2: mark cleared with the two caveats (non-listed positions pass literally on the bound host; unknown placeholders are not policed).
- Secrets lifecycle paragraph and the recreate alternative: state the live/restart split of `msb modify --secret` above, and that every start-capable `msb` call (`start`, `exec`, `restart`, `modify --restart`) needs the value in its environment. `MsbProvider` must resolve secrets for all four, not only `create_instance` and `start`.
- Recommend `--secret-violation-action block-and-log` as the default (it is already the default) and surface `runtime.log` `secret violation` lines in the UI; offer `block-and-terminate` as an opt-in hard-stop mode.
- Note the `exec.log` reflection caveat and keep echo-style hosts off the allowlist of secret-bound sandboxes.
- Interception no longer needs to be forced by the app for secrets, but policy-only sandboxes still need `--tls-intercept` (spike 01).

## Notes for Spike C (chained `--proxy`)

- The secret substitution happens inside microsandbox before the upstream leg, so a chained mitmproxy sees the real value in `Authorization`. Expect it in mitmproxy's flow content; `mitm_flowlog_addon.py` records no header values.
- Interception stays implicitly on whenever a secret exists, so the `--tls-upstream-ca-cert-for` path from spike 01 stays the way to trust mitmproxy's CA on the upstream leg.
- A `--tls-bypass` host never gets substitution; a bypassed host through the proxy sees the placeholder only.
- Run spike C's secret check against a host in the allowlist with `--tls-bypass` absent, and verify a mitmproxy-observed `Authorization` equals the dummy value.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `msb start` / `msb exec` / `msb restart` fails `host environment variable X is not set` | Every start re-reads the secret from the starting process's environment; a stopped sandbox is started implicitly by `exec` | Set each bound secret's value on every spawned `msb` command that may start the sandbox; never rely on a persisted value |
| Adding a secret to a running sandbox fails `cannot modify ... without a restart / secret X requires restart` | Only value rotation and host-list changes are live | Use `msb modify ... --restart` for add or option changes, or `--next-start` to queue them |
| `msb modify --secret NAME` fails `secret must be in format ENV[:OPTIONS]@HOST[,HOST...]` | The host list is mandatory even when only rotating the value | Repeat `@HOST[,HOST...]` and put the new value in the `modify` process environment |
| After `--secret-rm`, the guest still has `NAME=$MSB_NAME` and the placeholder goes upstream | The removal clears the injection rule, not the guest environment or placeholder | Treat removal as revocation of the value only; recreate the sandbox to remove the placeholder variable |
| Placeholder in `X-Other`, query or body to the bound host arrives unsubstituted | Only `headers=[...]`-listed fields are substituted; other positions on the bound host are not blocked | Add `query` or `body` (`--restart` required) or use the right header |
| Placeholder to a non-bound host returns 403 `secret_policy_denied` | By design, in any position, not only the listed ones | Bind the secret to the host, or use `passthrough=HOST` to send the placeholder unchanged |
| Plain HTTP to the bound host returns 403 `secret_policy_denied` | `require_tls_identity` is true; a placeholder is never sent in clear text | Use HTTPS |
| Bound host unreachable over HTTPS after `--tls-bypass` | Strict mode cannot inspect an uninspected connection | Do not bypass secret-bound hosts; if bypass is required, also set `--net-strict=false`, accepting the placeholder reaches the host unsubstituted |
| `block` action leaves no trace | Only `block-and-log` and `block-and-terminate` write the `secret violation` line | Use `block-and-log` when the UI shows violations |
| The real value appears in `sandboxes/<name>/logs/exec.log` | The upstream reflected it (echo or debug endpoint) into guest stdout | Keep reflecting endpoints off the allowlist; treat `exec.log` as sensitive |
| Spawned `msb` shows the value in its environment | By design, the value is supplied on the child process | Set it on the `Command` only; never in argv, `--env`, or the app's own environment |
