---
type: spike
title: "Spike: Dotted SSH Aliases and Dotted Sandbox Names"
feature: namespaced-ssh-alias
---
# Spike: Dotted SSH Aliases and Dotted Sandbox Names

Run on 2026-11-26 on the Windows development host against `podman-machine-default`, with the bundled Mutagen sidecar (v0.18.1, `src-tauri/binaries/mutagen-x86_64-pc-windows-msvc.exe`) and Git for Windows OpenSSH 10.2p1, plus Windows OpenSSH 9.5p2 (`C:\Windows\System32\OpenSSH\ssh.exe`). The commands are in `probe.sh` beside this file. The target was a throwaway container `probe-s1` from `localhost/sandbox-mio/base:10` holding a probe public key in `/etc/ssh/authorized_keys.container` and a `socat` listener on `127.0.0.1:7777` that prints `probe-ok`. Mutagen ran with a scratch `MUTAGEN_DATA_DIRECTORY`, so the app's daemon and the user's own were not touched. The probe container and every probe-created Mutagen session were removed afterwards, and the SSH files were restored to their original bytes (SHA-256 of `~/.ssh/config`, `~/.ssh/sandbox-mio/config` and `~/.ssh/known_hosts` equal before and after).

## S1: a dotted alias in a forward URL — CLEARED

A managed block `Host probe.mio` was appended to `~/.ssh/sandbox-mio/config`, shaped like the block `ensure_sandbox_alias` writes (`ProxyCommand podman --connection podman-machine-default exec -i probe-s1 mio-sshd`, `User root`, a probe `IdentityFile` and `UserKnownHostsFile`, `IdentitiesOnly yes`, `StrictHostKeyChecking accept-new`, keepalives).

- **`ssh -G probe.mio` resolves the block.** Output, verbatim: `user root`, `hostname probe.mio`, `proxycommand podman --connection podman-machine-default exec -i probe-s1 mio-sshd`. Windows OpenSSH 9.5p2 printed the same `hostname` and `proxycommand`.
- **A real connection works with the dotted alias.** `ssh -o BatchMode=yes probe.mio 'echo connected-as $(whoami) on $(hostname)'` printed `Warning: Permanently added 'probe.mio' (ED25519) to the list of known hosts.` and `connected-as root on 1f20e3773eea`. Windows OpenSSH printed `win-openssh-connected root`, exit 0.
- **`mutagen forward create --name=probe-fwd tcp:127.0.0.1:47811 probe.mio:tcp:127.0.0.1:7777` succeeds**, exit 0: `Created session fwrd_uweVNrcMGHB0wcuw8v991vsxf2yuBiEr5lNEGEkI1bh`. `forward list` showed `Destination: URL: probe.mio:tcp:127.0.0.1:7777`, `Connected: Yes`, `Status: Forwarding`. Mutagen's URL parser takes the dotted token as the alias, and the agent install ran through the `podman exec` ProxyCommand.
- **Data flows through the forward.** `curl telnet://127.0.0.1:47811` printed `probe-ok`, and the session then reported `Connections: 0 open, 1 total, 0 B outbound, 9 B inbound`.
- **`.mio` is not in the IANA root zone.** `https://data.iana.org/TLD/tlds-alpha-by-domain.txt` (`# Version 2026112503, Last Updated Thu Nov 26 07:07:01 2026 UTC`, 1436 lines) has no `MIO` line (`grep -ix mio` exit 1). The suffix stays `.mio`; `.invalid` is not needed. A later ICANN delegation is possible but would only matter if a user's DNS then resolved a sandbox alias, which the ProxyCommand never does.
- **Not verified:** Claude Desktop itself (a GUI client) was not run. Both OpenSSH builds and Mutagen accepted the alias, which covers the strict-client concern at the protocol level; an alias with a trailing label is an ordinary hostname form.

## S2: a sandbox with a dot in its name — FAILS at Mutagen session names

Mutagen rejects `.` in a session name, and the app names sessions after the sandbox. Verbatim output of the exact argv shapes the app builds:

- `forward create --name=a.b ...` fails with `Error: invalid session name: invalid name character at index 1: '.'`.
- `forward create --name=agentweb-a.b ...` (the agent-web forward, `forward_name` in `src-tauri/src/network/mod.rs`) fails with `Error: invalid session name: invalid name character at index 10: '.'`.
- `forward create --name=expose-a.b-8080 ...` (an exposed port) fails with `Error: invalid session name: invalid name character at index 8: '.'`.
- `sync create --name=a.b <dir> probe.mio:/tmp/x` (the deploy session, named by `SyncEngine::create` with the bare sandbox name in `src-tauri/src/services/sandbox.rs`) fails with the same index-1 error. With `--name=a-b` the same call succeeds, so the dotted alias is not the cause.

The accepted name grammar was probed too: a name must start with a letter and may then hold only letters, digits and `-`. Accepted: `a-b`, `A1`, `ab1`, `a-`. Rejected: `a_b` and `a_` (`invalid name character at index 1: '_'`), `1ab` and `-ab` (`name does not start with Unicode letter`).

Everything else a dotted name touches works. On the same host a container `spike.dot`, a sidecar `spike.dot-proxy` and a network `mio-proxy-spike.dot` were created and used, and `herdr --session spike.dot status server` reported `running` with its socket under `sessions/spike.dot/`. The terminal path (russh over `podman exec`, container-facing calls keyed on the bare name) and the workspace path `<root>/a.b` contain nothing that parses the dot.

Consequences for the in-app flows with a dotted name: the terminal pane works; the agent-web pane and every expose-port call fail when the forward is created; deploy fails at sync-session creation; destroy was not exercised (it only terminates sessions of those names, which never exist), and a dotted sandbox could not use half the app. A full in-app run was not driven: the failing step is the Mutagen CLI itself on the argv above, and the only code between the sandbox name and that argv is string formatting, so an app run would reproduce the same error.

## Findings that change the plan

- **Resolved Decision 5.** Keep `NameHasDot`, reworded to cite Mutagen session names instead of SSH hosts. Lifting the ban would need every Mutagen session name to be derived from the sandbox name by an encoding that cannot collide (`a.b` against `a-b`), which is a larger change than this feature.
- **Pre-existing, independent of the dot.** `validate_name` still allows `_` and a leading digit. Mutagen rejects `_` in any session name, and `sync create` is named with the bare sandbox name, so an existing sandbox named `my_box` or `2fast` cannot use agent-web, expose-port or deploy today. Recorded in `04_improvements.md`.
- **A seam the plan missed.** `expose_port` in `src-tauri/src/network/mod.rs` passes the bare sandbox name to `forward_create` as the alias, not the route's `forward_alias`, so Phase B must derive the alias there too. `expose_port` takes no alias, so it needs the derivation or an alias parameter.
