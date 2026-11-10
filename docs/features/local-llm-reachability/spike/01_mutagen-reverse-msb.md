---
type: spike
title: "Spike 01: Mutagen Reverse Forward into a microsandbox Guest"
feature: local-llm-reachability
---
# Spike 01: Mutagen Reverse Forward into a microsandbox Guest

This spike tests whether the app's bundled Mutagen can create a reverse forward whose listener sits on a microsandbox guest's loopback and whose destination is the Windows host's llama-server. Spike 01 ran on 2026-11-10 on the Windows 11 development host with Mutagen 0.18.1 and `msb 0.7.12`. The app's private Mutagen data directory (`%APPDATA%\com.tincho.sandbox-mio\mutagen`) was used, so the probe used the app's own daemon and the app-written SSH alias `rrr`.

## Diagnosis Run (2026-11-09)

The diagnosis ran inside the user's microsandbox sandbox `rrr`, which has the traffic viewer on (`rrr-proxy` sidecar, chained SOCKS5) and an `open` `.mio/net-policy.json`. It established the failure this feature fixes.

| Check | Result |
|---|---|
| Host llama-server `http://127.0.0.1:56306/v1/models` from Windows | PASS |
| Podman-machine listener `127.0.0.1:51199` from inside `podman-machine-default` | PASS |
| `inference-reverse` Mutagen session | `Forwarding`, `0 total` connections |
| `getent hosts host.containers.internal` inside `rrr` | no answer; curl reports `Could not resolve host: host.containers.internal` |
| `/etc/hosts` inside `rrr` | has `host.microsandbox.internal` (`172.16.0.9`) only |
| `curl host.microsandbox.internal:51199` and `:56306` inside `rrr` | both fail (curl status `000`) |
| `msb inspect rrr` effective policy | `default_egress: deny`, `allow host` on port 53 only, `allow public`, `outbound_proxy` SOCKS5 to the sidecar |
| `/root/work/.mio/pi-agent/models.json` | `baseUrl` `http://host.containers.internal:51199/v1`; model list predates the 2026 catalogue refresh and lacks the running `qwen3.5-2b` |

## Probe (2026-11-10)

The probe ran `mutagen forward create --name=spike-inference-rrr rrr:tcp:127.0.0.1:51199 tcp:127.0.0.1:56306`. Sandbox `rrr` was stopped at the time; its earlier `expose-rrr-8082` forward had already been removed, which suggests an app restart had happened.

Blocker, verbatim:

```
Error: unable to connect to source: unable to connect to endpoint: unable to dial agent endpoint: unable to handshake with agent process: unable to receive server magic number: EOF (error output: error: invalid config: SOCKS5 proxy: host environment variable MIO_SOCKS_PASSWORD is not set
Connection closed by UNKNOWN port 65535)
```

An immediate `msb exec rrr -- ...` then reported `error: sandbox 'rrr' is in state Starting and cannot be started`; `msb list` afterwards showed `rrr` `stopped`. The probe session was not left behind; Mutagen had not registered it.

## Rerun Against a Running Guest (2026-11-10)

The user started `rrr` from the app. The inference server had restarted meanwhile on a new host port (`55009`), and the app's `inference-reverse` session had followed it. The probe was `mutagen forward create --name=spike-inference-rrr rrr:tcp:127.0.0.1:51199 tcp:127.0.0.1:55009`.

| Check | Result | Evidence |
|---|---|---|
| Session creation | PASS in 1.5 s | `Created session`; both endpoints `Connected: Yes`, `Status: Forwarding` |
| Guest listener | PASS, guest loopback only | `/proc/net/tcp` row `0100007F:C7FF ... 0A`; the listener belongs to a `mutagen-agent forwarder` process in the guest |
| `curl http://127.0.0.1:51199/v1/models` inside `rrr` | PASS | HTTP 200, llama.cpp model metadata |
| `/v1/chat/completions` inside `rrr` (20 tokens) | PASS | HTTP 200 in 4.0 s |
| Same traffic under `rrr`'s policy (`default_egress: deny`, TLS interception, chained SOCKS5) | PASS | Nothing about the traffic appeared in `rrr`'s sidecar state directory (`%APPDATA%\com.tincho.sandbox-mio\proxy\rrr`); guest loopback never reaches the policy or the chain |
| pi 1.5.0 end to end, with a copy of `.mio/pi-agent` whose `baseUrl` was `http://127.0.0.1:51199/v1` | PASS | `pi -p --provider local-llm --model qwen3-4b-instruct` printed `pong`; the session's connection count rose |
| pi run from inside `/root/work` with `PI_CODING_AGENT_DIR` exported | FAIL, harness artefact | `Connection error.`; the workspace's `mise.toml` env re-sets `PI_CODING_AGENT_DIR` to `/root/work/.mio/pi-agent`, which overrides the export, so pi read the real config with the `host.containers.internal` URL |
| App's port poller | Auto-exposed the guest listener | A session `expose-rrr-51199` appeared within seconds, and the app removed it once the spike session was terminated |

## Stop Behaviour (2026-11-10)

`msb stop rrr` was run from the CLI with the spike session live, then `msb list` was polled every 3 s for 60 s.

| Observation | Result |
|---|---|
| With the spike session live | `rrr` cycled `stopped` → `starting` → `stopped` about every 24 s. The session reported `Status: Connecting to source` with `Last error: ... error: sandbox 'rrr' is in state Starting and cannot be started` |
| After `mutagen forward terminate spike-inference-rrr` | The cycling continued. The app log showed the app's port poller running `msb exec rrr -- sh -c cat /proc/net/tcp ...` every 2 s against the stopped sandbox, failing intermittently with `error: sandbox 'rrr' is in state Starting and cannot be started` |
| Plain `msb exec rrr -- true` from the CLI | `error: invalid config: SOCKS5 proxy: host environment variable MIO_SOCKS_PASSWORD is not set` |

The app was not told about the CLI stop, so its poller kept running. That confounds the 60 s timeline. The first probe settles the Mutagen question on its own: it ran while the app was not polling `rrr` and still produced a boot attempt.

## Findings

| Risk | Outcome |
|---|---|
| Mutagen reverse forward listens on a microsandbox guest's loopback and reaches the host | CLEARED: listener on guest `127.0.0.1`, HTTP 200 for models and chat, and pi answered through it |
| Deny-egress policy, TLS interception or chained SOCKS proxy interferes | CLEARED: none applies to guest loopback |
| A Mutagen dial against a stopped microsandbox sandbox | The alias's `ProxyCommand` (`msb exec --stream <name> -- mio-sshd`) tries to boot the stopped VM. Without the app's start environment (`MIO_SOCKS_PASSWORD`, secret values from `src-tauri/src/services/start_env.rs`), the boot fails for a proxied sandbox. Mutagen's reconnect loop repeats the dial, so a live session keeps trying to boot a stopped sandbox |
| Forward's guest listener auto-exposed as a user port | CONFIRMED: the session must be tracked by `NetworkManager` so the port poller excludes it |
| Host port changes between server runs | CONFIRMED: the server moved from `56306` to `55009` between 2026-11-09 and 2026-11-10 |

Clean-up: the spike session was terminated; `rrr`'s real `.mio/pi-agent` and `mise.toml` were not modified; the throwaway pi config at `/tmp/pi-spike` inside `rrr` is lost when the VM restarts.
