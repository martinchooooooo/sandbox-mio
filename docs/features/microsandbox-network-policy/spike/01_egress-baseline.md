# Spike 01: Default-Deny Egress Baseline on msb 0.7.12

Hands-on spike run 2026-11-01 on the Windows 11 development host against `msb 0.7.12` (`C:\Users\krist\.microsandbox\bin\msb.exe`, upgraded from 0.6.6, `msb doctor` passing). It clears Validation items 1, 4 and 5 of `docs/features/microsandbox-network-policy/01_ideas.md`; items 2 and 3 (`--secret`, chained `--proxy`) are left for later spikes. All commands ran from Git Bash. Every sandbox (`spike-netpol-*`), the spike volume, the spike image tag and the spike SSH key were removed at the end.

## Setup

**Base image.** `sandbox-mio/base:6` (the current `BASE_IMAGE_VERSION`, `src-tauri/src/backends/mod.rs`) was already in the msb image store from the 0.6 install and booted unchanged on 0.7.12. Podman was not running, so the project's `podman save` → `msb load` path (`MsbProvider::ensure_base_image`) could not be replayed end to end. The `msb load` half was verified with a round trip instead: `msb save sandbox-mio/base:6 --output <tar>` then `msb load --input <tar> --tag spike-netpol/base:6` succeeded (`Loaded sandbox-mio/base:6`, `Loaded spike-netpol/base:6`).

**Policy under test.**

```
msb create sandbox-mio/base:6 --name spike-netpol-b \
  --mount-named "spike-netpol-ws:/root/work:kind=disk,size=2G" \
  --net-default-egress deny --net-rule allow@dns \
  --net-rule "allow@registry.npmjs.org:tcp:443" --net-rule "allow@github.com:tcp:443" \
  --net-rule "allow@api.github.com:tcp:443" --net-rule "allow@*.githubusercontent.com:tcp:443" \
  --net-rule "allow@api.anthropic.com:tcp:443" --net-rule "allow@nodejs.org:tcp:443" \
  --net-strict --tls-intercept
```

`create` prints nothing and exit 0; the sandbox is already running (`msb ls`: `running`).

## Item 1: Policy Still Runs the Workflow

| Check | Result | Evidence |
|---|---|---|
| Strict + `--tls-intercept`: HTTPS to an allowed host | PASS | `curl https://registry.npmjs.org/left-pad` → `HTTP/1.1 200 OK`; the guest trusts the interception CA automatically (`NODE_EXTRA_CA_CERTS=/.msb/tls/ca.pem`, `CURL_CA_BUNDLE`, `SSL_CERT_FILE` are set by msb) |
| Strict without `--tls-intercept` | FAIL (as 01_ideas predicts) | `curl: (35) OpenSSL SSL_connect: SSL_ERROR_SYSCALL in connection to registry.npmjs.org:443`; reproduced with explicit `--net-strict` and with the flag omitted, because strict defaults to true |
| `mise use -g node@lts` (downloads from nodejs.org, then `npm view left-pad version`) | PASS | node 24.22.2 installed, `mise exec -- npm view left-pad version` → `1.3.0`. One warning: `mise-versions.jdx.dev` returned 403 (not allow-listed); mise falls back to nodejs.org and succeeds |
| `git ls-remote https://github.com/jdx/mise HEAD` | PASS | returned the HEAD sha |
| Agent API reachable | PASS | `curl -X POST https://api.anthropic.com/v1/messages` → `401`, `{"type":"error","error":{"type":"authentication_error","message":"x-api-key header is required"}...}` |
| Denied host over HTTP | PASS | `curl http://example.com` → `403`, `Content-Type: application/json`, body below |
| Denied host over HTTPS with interception | PASS | same JSON 403 body |
| Denied host over HTTPS without interception | PARTIAL | no readable body; connection fails (`curl: (7) Failed to connect to example.com port 443 after 17 ms`) |
| Allowed host on a non-allowed port | PASS | `http://registry.npmjs.org/` (rule is `:tcp:443`) → the same JSON 403 |
| `msb ssh <name> -- echo ssh-ok` | PASS | prints `ssh-ok`, exit 0; the shell inside is then still policy-bound (`example.com` → 403 JSON) |
| `msb ssh serve <name> --stdio` as an OpenSSH `ProxyCommand` | PASS | after `msb ssh authorize --file <pub>`: `ssh -o "ProxyCommand=<msb.exe> ssh serve spike-netpol-b --stdio" root@spike-netpol-b 'echo stdio-ok; uname -a'` printed `stdio-ok` and `Linux spike-netpol-b 6.12.113` |
| The app's real transport `msb exec --stream <name> -- mio-sshd` (what `msb_proxy_command` in `src-tauri/src/backends/mod.rs` builds) | PASS | `ssh`, `scp` of a 3 MB random file (sha1 identical on both ends, landed on the ext4 `/root/work` disk volume) and `sftp ls` all worked under default-deny |
| Mutagen sync | NOT RUN | Mutagen 0.18.1 resolves the beta through the user's `~/.ssh/config` alias. Editing that file was refused by the permission layer and `MUTAGEN_SSH_PATH` needs an `ssh.exe`, so a throwaway alias could not be injected. The transport Mutagen uses (`scp`/`sftp` over `mio-sshd`) is proven above; the agent's reverse-forward leg is covered by item 4 |

Denied-request body (identical for HTTP and intercepted HTTPS; also for a raw IP, with `"domain":"1.1.1.1"`):

```
HTTP/1.1 403 Forbidden
Content-Type: application/json
Cache-Control: no-store
{"code":"network_policy_denied","domain":"example.com","message":"Request blocked by network policy."}
```

### Strict and interception matrix

All variants use the same allow rules, `--net-default-egress deny`, and no workspace volume.

| Variant | Allowed HTTPS | Denied HTTPS | Mismatched `Host` header to an allowed IP |
|---|---|---|---|
| `--net-strict` (default), no intercept | fails (SSL_ERROR_SYSCALL) | connect fails | not reached |
| `--net-strict=false`, no intercept | 200 | connect fails (no body) | 403 (origin-side) |
| `--tls-intercept --net-strict=false` | 200 | JSON 403 | connection closed, `Empty reply from server` |
| `--tls-intercept --net-strict` | 200 | JSON 403 | `curl -H "Host: example.com"` → connection closed (`000`); `--resolve example.com:443:<allowed ip>` → 403 JSON |

Verdict for item 1: PASS for policy, `mise install`, git, agent API reach, SSH terminal and stdio transport. PARTIAL overall only because the Mutagen session itself was not created.

## Item 4: Reverse Forward Under Default-Deny

`forward_create_reverse` (`src-tauri/src/sync/mod.rs`) runs `mutagen forward create --name=<n> <alias>:tcp:0.0.0.0:<guest_port> tcp:127.0.0.1:<host_port>`. Mutagen's agent runs inside the guest, reached over the same `mio-sshd` transport, listens in the guest and dials the Windows host loopback. Because Mutagen could not be driven (item 1), the identical topology was reproduced with an SSH remote forward over the same `mio-sshd` transport.

```
python -m http.server 8099 --bind 127.0.0.1        # host side, serves "from-host-llm"
ssh -F spike_ssh_config -N -f -R 127.0.0.1:18080:127.0.0.1:8099 spike-netpol-b
msb exec spike-netpol-b -- curl -sS -m 10 http://127.0.0.1:18080/v1   # → from-host-llm
```

| Check | Result | Evidence |
|---|---|---|
| Guest loopback listener forwarded to a host port answers under `--net-default-egress deny` + strict + intercept | PASS | `from-host-llm` returned; guest loopback traffic never touches the egress policy |
| Guest dials the host directly via `host.microsandbox.internal:8099` | FAIL (expected) | `curl: (7) Failed to connect ... after 0 ms`. The default-deny policy blocks the host group; the reverse forward is therefore the only host path, and a direct route would need an explicit host allow rule |

Verdict for item 4: PASS. Not tested: Mutagen's own `0.0.0.0` guest bind (the app's source address); loopback is not egress, so no behavioural difference is expected.

## Item 5: `statx` on a Windows Bind Mount (issue #1219)

```
msb create sandbox-mio/base:6 --name spike-netpol-f --workdir /root --mount-dir "C:\...\tmp\bind:/w"
msb exec spike-netpol-f -- sh -c 'cat /w/f.txt; stat /w/f.txt; ls -l /w; touch /w/g'
```

| Check | 0.6.6 (`hands-on-cli.md`) | 0.7.12 |
|---|---|---|
| `stat /w/f.txt` | `cannot statx ... Too many levels of symbolic links`, exit 1 | PASS: full stat output (`Size: 5`, mode `0777`, owner root), exit 0 |
| `cat` / `ls -l` / `touch` on the mount | content correct, exit 1 (#1218 `close()` EACCES) | PASS: all exit 0 |
| `msb exec ... cat /w/f.txt` host-side exit code | 1 | 0 |
| `git init` + `git status` in the mount | not recorded | PASS |

Verdict for item 5: PASS (no `ELOOP`). Both #1219 and the #1218 exit-code symptom are gone for a `--mount-dir` bind mount on 0.7.12. This reopens the `host_bind_mount` capability probe of `docs/features/microsandbox-provider/02_dev.md` and belongs in that feature's `04_improvements.md`. Not tested: Inotify propagation, file locking (#949), performance. The inspect output now shows a `stat_virtualization` field on each mount (`strict` here).

## 0.7.12 Differences from `01_ideas.md`

| `01_ideas.md` says | 0.7.12 reality |
|---|---|
| `--net-strict` is an opt-in flag | It is `--net-strict[=<true\|false>]` and defaults to true. A policy-less create is already strict, and a deny-egress sandbox without `--tls-intercept` therefore cannot make HTTPS calls at all |
| `--secret NAME[:OPTIONS]@HOST[,HOST...]`, value read from env var named `NAME` (create help) | The first token is the host environment variable name `ENV`. Inline `ENV=VALUE@HOST` is rejected. Options are `no-headers`, `headers=[FIELD,...]` (also `headers=*`), `query`, `body`, `passthrough=HOST` or `passthrough=[HOST,...]`; headers are substituted by default. `msb modify --secret` uses the same grammar and adds `--secret-rm <NAME>` |
| Proxy auth "reads credentials from a host environment variable" | Create flags: `--proxy <http://IP:PORT\|socks4://\|socks5://>`, `--socks5-username`, `--socks5-password-env`, `--socks4-user-id`, `--http-connect-username`, `--http-connect-password-env`. Note `--proxy` takes an IP:PORT form in help |
| Policy file needs an explicit DNS allowance | `allow@dns` is not needed and is harmful: without it, the gateway DNS resolves only allowed names (`example.com` → `Could not resolve host`, allowed `registry.npmjs.org` resolves). With `allow@dns`, denied hosts still resolve, which leaves a DNS exfiltration channel. Drop `allow@dns` from the generated rules |
| "Readable denials: JSON 403 for HTTP and intercepted HTTPS" | Confirmed (body above, default on, `deny_response_format: json` visible in `inspect`). Plain HTTPS without interception gets no body, only a failed connect |
| Starter allowlist for mise: npm, GitHub, agent API | Also needs `nodejs.org` for Node, and `mise-versions.jdx.dev` to avoid a 403 warning on every `mise use` |
| Rule grammar `--net-rule` tokens | Confirmed: `<action>[:<direction>]@<target>[:<proto>[:<ports>]]`, `*.example.com` suffix needs at least two labels, groups include `public`, `private`, `multicast`, `host`. Extras not in 01_ideas: `--no-net`, `--net <profile>`, `--net-default[-ingress]`, bandwidth and ops limits, `--max-tcp-connections`, `--tls-bypass`, `--tls-intercept-port`, `--tls-no-verify-upstream-for`, `--no-block-quic` (QUIC is blocked by default when intercepting), `--trust-host-cas`, `--no-dns-rebind-protection`, `--security restricted` |
| `msb modify` has no network-rule flag | Confirmed by `msb modify --help`: it lists `--secret` and `--secret-rm` but no `--net-*` option |
| Policy readback not specified | `msb inspect <name> --format json` exposes the effective policy under `active_config.network.policy` (default_egress, ordered rules with `destination.domain` or group, ports, protocols) and `active_config.network.http.deny_response`; the `allow@dns` rule is shown as `{group: host, ports 53, udp+tcp}`. Useful for a "show effective policy" panel |
| A disk volume can be re-used by another sandbox | A `--kind disk` volume admits one attached sandbox: a second `msb create` against `spike-netpol-ws` failed with `invalid config: volume "spike-netpol-ws" is already attached with an incompatible disk mode` while the first still existed. Matters for recreate (`SandboxBackend::recreate`): remove the old sandbox before creating the new one |

## 0.6 → 0.7 Breaking Changes Affecting `MsbProvider`

Each `src-tauri/src/provider/msb.rs` call was replayed on 0.7.12. No Rust code was edited.

| `msb.rs` call | Result on 0.7.12 |
|---|---|
| `ls --format json` (`list_instances`) | Compatible: array of `{created_at, image, name, status}`; status strings are still PascalCase (`Running`, `Stopped`), so `map_status` holds |
| `image ls --format json`, `reference` exact match (`image_exists`) | Compatible; the store lists both `sandbox-mio/base:6` and `localhost/sandbox-mio/base:6` |
| `inspect <name> --format json`, `active_config.labels` (`workspace_label`) | Compatible; labels are under `active_config.labels`. The image's own build label (`io.buildah.version`) also appears there |
| `volume create <n> --kind disk --size <s>`; taken-name error | Compatible; `error: volume already exists: spike-netpol-ws`, exit 1, contains the substring `ensure_volume` matches |
| `create <image> --name --mount-named <vol>:/root/work:kind=disk,size=... --label` | Compatible on a fresh volume. New: the create validates the image workdir in the guest. The base image's workdir is `/root/work`, so a create without that mount fails with `could not validate workdir in guest "/root/work": ... No such file or directory`; tests without a volume need `--workdir /root` |
| `start` on a running sandbox (`start`) | Compatible; `error: sandbox still running: cannot start sandbox '...': already running`, exit 1, still contains `already running` |
| `stop`, `rm --force`, `volume rm` | Compatible; `rm --force` of a missing name still exits 1 (`sandbox not found`) |
| `exec --stream --workdir --env <name> -- argv`, exit-code propagation, `bash -c '... & disown'` detached wrapper | Compatible; exit 7 propagated |
| `load --input <tar> --tag <ref>` | Compatible; output now prints a `Loaded` line for each tag |
| `status`/`ps` | Takes `-a/--all`; `msb ls` has no `-a` (`error: unexpected argument '-a' found`). `MsbProvider` uses `ls` without `-a`, which already lists stopped sandboxes |

No argv or output-format break was found for the calls the provider makes today. The 0.7 differences that matter are additive: workdir validation at create, exclusive disk-volume attachment, and mount stat-virtualization fields in `inspect`.

## Implications for Later Spikes

- **Secrets (item 2).** The create help states the value is read from the host variable named by `ENV` and "stored only as a source reference, never inlined in the sandbox config". Verify with `msb inspect` that no value appears. Interception must be on; `--tls-intercept` already works with the generated CA, with no guest-side step.
- **Chained proxy (item 3).** `--proxy` takes `IP:PORT` and the SOCKS5 password comes from `--socks5-password-env`. Under strict mode the interception layer stays on, which is the precondition for the proposed layering. `--tls-upstream-ca-cert-for <PATTERN=PATH>` exists for per-host trust of mitmproxy's CA.
- **Do not combine** `allow@dns` with an allowlist if DNS exfiltration matters; domain-allow rules alone resolve exactly the allowed names.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| HTTPS fails with `SSL_ERROR_SYSCALL` under `--net-default-egress deny` although the host is allowed | `--net-strict` is on by default and the authority cannot be inspected without `--tls-intercept` | Always pair deny-egress with `--tls-intercept`, or pass `--net-strict=false` and accept no readable denial |
| Denied HTTPS shows `connection failed`, not the JSON 403 | JSON denial is only synthesised for HTTP and intercepted HTTPS | Enable `--tls-intercept` when the agent should see the reason |
| Denied hostnames still resolve in DNS | `--net-rule allow@dns` opens resolver access for every name | Omit `allow@dns`; allowed domain rules resolve on their own |
| `mise use` warns `mise-versions.jdx.dev ... 403` | The host is not allow-listed | Add `mise-versions.jdx.dev` to the starter list, together with `nodejs.org` |
| `msb create` fails `could not validate workdir in guest "/root/work"` | 0.7 checks the image workdir; the base image expects the workspace mount | Mount the workspace volume at `/root/work` or pass `--workdir` |
| Second sandbox cannot use the same `kind=disk` volume | Exclusive attach: `already attached with an incompatible disk mode` | `msb rm` the old sandbox before recreating around the volume |
| `msb ssh authorize --file /c/...` reports `failed to read` | Git Bash paths are not Windows paths for `msb.exe` | Pass `cygpath -m` paths; also `export MSYS_NO_PATHCONV=1` or `--workdir /root` becomes `C:/Program Files/Git/root` |
| `ssh ... msb.exe ssh serve --stdio` returns `Permission denied` | The key is not in `%USERPROFILE%\.microsandbox\ssh\authorized_keys` | `msb ssh authorize --file <pub>`; remove the line after a test |
| Mutagen cannot be pointed at a throwaway SSH alias | It reads the user's `~/.ssh/config`; `MUTAGEN_SSH_PATH` needs an `ssh.exe`, not a `.cmd` | Test sync via the app's own managed alias, or accept the `scp`/`sftp` proof |
