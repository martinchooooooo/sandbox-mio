---
type: spike
title: "Spike S2: Non-Interactive PATH over the Exec Transport"
feature: host-tool-ssh-access
---
# Spike S2: Non-Interactive PATH over the Exec Transport

This spike decides the Phase B mechanism of `docs/features/host-tool-ssh-access/02_dev.md`: how a non-interactive SSH session over the exec transport (`sshd -i` launched by `/usr/local/bin/mio-sshd` through `podman exec`) gets the sandbox's mise-managed tools on `PATH`. It also records the headless part of S4 (what an abruptly ended client leaves in the container). It ran on 2026-11-18 on the Windows host, with the system client `C:\WINDOWS\System32\OpenSSH\ssh.exe` (`OpenSSH_for_Windows_9.5p2`) driven from PowerShell, against throwaway containers of `localhost/sandbox-mio/base:8` (Debian 12, `OpenSSH_9.2`, mise `2026.6.14`) on `podman-machine-default`. No app, no user sandbox and no `~/.ssh` file was touched; the containers, the spike image tag and the keypair were removed afterwards.

**Outcome: `SetEnv PATH=/root/.local/share/mise/shims:<root's default path>` in the baked `sshd_config.container`.** It was confirmed on a rebuilt image. `/root/.bashrc` *is* read for a remote command under `sshd -i`, so the plan's `.bashrc` alternative also works; it lost on independence from shell startup files, not on correctness.

## Setup

- Container: `podman run -d --name mio-spike-s2-<id> localhost/sandbox-mio/base:8 sleep infinity`, then `podman cp <throwaway>.pub <name>:/etc/ssh/authorized_keys.container`, the same install `install_sandbox_key` does.
- Tool install, mirroring `MiseRuntime::init` and `install` (`src-tauri/src/runtime/mod.rs`): write `/root/work/mise.toml` as `[tools]` with an empty `mise.lock`, then `podman exec -w /root/work <name> mise use node@24.19.0`. The shims directory `/root/.local/share/mise/shims` holds `node`, `npm`, `npx` and `corepack` afterwards.
- Client: `ssh -F <spike config> <name> …`, where the spike config holds one `Host` block shaped like the one `ensure_sandbox_alias` (`src-tauri/src/backends/mod.rs`) writes: `ProxyCommand "C:/Users/<user>/AppData/Local/Programs/Podman/podman.exe" --connection podman-machine-default exec -i <name> mio-sshd`, `User root`, quoted `IdentityFile` and `UserKnownHostsFile` under a temp directory, `IdentitiesOnly yes`, `StrictHostKeyChecking accept-new`. `-F` keeps `~/.ssh/config` out of the run.
- Probes: `s2-probe.sh` (copied to `/tmp`) runs `node` from `/root`, `/root/work` and `/root/work/sub` without a shell `cd` hook; `s2-interactive-input.txt` is fed to `ssh -tt` through `cmd /c "… < file"` (PowerShell's pipe adds a BOM) to drive an interactive login shell. A marker line `echo BASHRC-MARKER-READ >&2` was appended to `/root/.bashrc` for the baseline and candidate runs, not for the rebuilt-image run.

Baseline: `ssh -F <cfg> <name> whoami` printed `root`.

## Baseline (image 8, unchanged)

```
### login shell, home            (ssh <name> 'command -v node || echo node NOT FOUND; echo PATH=$PATH')
node NOT FOUND
PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
BASHRC-MARKER-READ
### login shell after cd /root/work      (ssh <name> 'cd /root/work; command -v node; node -v')
BASHRC-MARKER-READ
/root/.local/share/mise/installs/node/24.19.0/bin/node
v24.19.0
### child process (agent position)       (ssh <name> /tmp/probe.sh)
BASHRC-MARKER-READ
probe SHLVL=1 PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
  /root        node: NOT FOUND (/tmp/probe.sh: line 6: node: command not found)
  /root/work   node: NOT FOUND (/tmp/probe.sh: line 6: node: command not found)
  /root/work/sub node: NOT FOUND (/tmp/probe.sh: line 6: node: command not found)
```

The session environment (`env | sort` over SSH):

```
HOME=/root  LOGNAME=root  MAIL=/var/mail/root  PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
PWD=/root  SHELL=/bin/bash  SHLVL=1  USER=root  MISE_SHELL=bash  __MISE_*=…
SSH_CLIENT=UNKNOWN 65535 65535
SSH_CONNECTION=UNKNOWN 65535 UNKNOWN 65535
```

What the baseline establishes:

- **`~/.bashrc` is read for a remote command under `sshd -i`.** The marker prints on every non-interactive command. The plan's concern was that bash's remote-command special case keys on `SSH_CLIENT`, which an `sshd -i` over a pipe might not set; it does set it, to `UNKNOWN 65535 65535`, and Debian's bash is built with `SSH_SOURCE_BASHRC`, so the special case triggers.
- **Reading `.bashrc` is not enough.** Today's `.bashrc` runs the full `mise activate bash`, whose hook resolves tools for the shell's *current* directory and re-runs only on the wrapped `cd`/`pushd`/`popd` and on a prompt. sshd starts every session in `$HOME` (`/root`), outside the workspace, so the session's `PATH` carries no tool. A login shell that `cd`s into `/root/work` gets node, frozen at that version; any process started with a working directory but no shell `cd` (how a remote agent spawns tools) does not.
- **The image `ENV` does not reach an SSH session.** `MISE_DATA_DIR`, `MISE_YES` and `MISE_LOCKFILE` are absent: sshd builds the session environment from scratch. mise still finds its data, because its default data directory is the same `/root/.local/share/mise`, and the workspace config was already trusted by the `mise use` the install ran (`mise trust --show` printed `~/work: trusted`).
- `mise --version` works everywhere: mise itself is in `/usr/local/bin`.

## Candidate (a): `SetEnv PATH` in `sshd_config.container`

Appended `SetEnv PATH=/root/.local/share/mise/shims:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin`; `sshd -t -f /etc/ssh/sshd_config.container` accepted it, and every connection reads the file afresh, so no restart was involved.

```
### login shell, home
/root/.local/share/mise/shims/node
mise ERROR No version is set for shim: node
PATH=/root/.local/share/mise/shims:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
### login shell after cd /root/work
/root/.local/share/mise/installs/node/24.19.0/bin/node
v24.19.0
PATH=/root/.local/share/mise/installs/node/24.19.0/bin:/root/.local/share/mise/shims:/usr/local/sbin:…
### child process (agent position)
probe SHLVL=1 PATH=/root/.local/share/mise/shims:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
  /root        /root/.local/share/mise/shims/node mise ERROR No version is set for shim: node
  /root/work   /root/.local/share/mise/shims/node v24.19.0
  /root/work/sub /root/.local/share/mise/shims/node v24.19.0
### exec'd non-shell child                (ssh <name> 'exec /usr/bin/env')
PATH=/root/.local/share/mise/shims:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
### non-interactive login shell child     (ssh <name> 'bash -lc /tmp/probe.sh')
probe SHLVL=1 PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/root/.local/share/mise/shims
  /root        /root/.local/share/mise/shims/node mise ERROR No version is set for shim: node
  /root/work   /root/.local/share/mise/shims/node v24.19.0
  /root/work/sub /root/.local/share/mise/shims/node v24.19.0
### interactive login (ssh -tt, input file)
FLAGS=himBHs SHLVL=1
LOGIN-PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
WORK-PATH=/root/.local/share/mise/installs/node/24.19.0/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
v24.19.0
HOME-PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
```

- OpenSSH 9.2's `sshd` honours `SetEnv PATH` in inetd mode, overriding its compiled-in root path; `PermitUserEnvironment` plays no part (it governs `~/.ssh/environment`, not `SetEnv`).
- The shims resolve the tool version from the directory the tool *runs in*, so a long-lived agent started in `/root` still runs the right node in `/root/work` and below, and would follow a different `mise.toml` in another directory.
- A login shell's `/etc/profile` resets `PATH` for uid 0. In an interactive login that leaves exactly today's behaviour (`mise activate` takes over on `cd`). In a non-interactive login (`bash -lc`) the shims survive at the *end* of `PATH`, carried by mise's inherited `__MISE_ORIG_PATH`; harmless while the image has no system `node`.
- The `sftp` subsystem still works (`sftp -b` listed `/root/work`), so Mutagen's `scp` agent install is unaffected.

## Candidate (b): a non-interactive shims branch in `/root/.bashrc`

`sshd_config.container` restored, `.bashrc` replaced by an `if [[ $- == *i* ]]` branch: `eval "$(mise activate bash)"` when interactive, `eval "$(mise activate bash --shims)"` (which emits `export PATH="/root/.local/share/mise/shims:$PATH"`) otherwise.

The same probes gave the same results as (a) in every position: shims first on `PATH` for the login shell, the agent-position child, an exec'd `env`, `sh -c` and `bash -lc` children (for `bash -lc` the shims stay *first*, because `.profile` sources `.bashrc` again after `/etc/profile`), and the interactive login unchanged (`LOGIN-PATH` without shims, `WORK-PATH` from `mise activate`).

## Why (a) over (b)

Both pass. (a) is the one Phase B should ship:

- It is independent of the shell and of its startup files. (b) works only because sshd still exports `SSH_CLIENT` over a pipe *and* Debian builds bash with `SSH_SOURCE_BASHRC`; neither is a contract.
- It costs nothing per command. (b) adds a `mise activate --shims` process to every SSH command, on top of the `mise activate bash` that already runs.
- It touches only SSH sessions. Herdr panes are spawned by a Herdr server that the app starts through the engine's exec, not SSH (`create_argv` in `src-tauri/src/terminal/mod.rs`); the server's environment on the rebuilt image was the image `ENV` (`PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin`, `MISE_DATA_DIR`, `MISE_LOCKFILE`, `MISE_YES`, cwd `/root/work`), so panes keep today's interactive `mise activate` behaviour. The Herdr *client* the app attaches over SSH does get the shims on `PATH`, which it never uses.

(b)'s one advantage, shims first rather than last under `bash -lc`, matters only if a system copy of a tool ever shadows a mise one.

## Confirmation on a rebuilt image

The Containerfile with one added `printf` line (below) built as a spike-only tag `localhost/mio-spike-s2:setenv`, run with the unmodified `.bashrc`:

```
### whoami
root
### S2 command from the plan        (ssh <name> 'which node; mise --version 2>/dev/null; echo $PATH')
/root/.local/share/mise/shims/node
2026.6.14 linux-x64 (2026-06-25)
/root/.local/share/mise/shims:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
### child process (agent position)
probe SHLVL=1 PATH=/root/.local/share/mise/shims:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
  /root        /root/.local/share/mise/shims/node mise ERROR No version is set for shim: node
  /root/work   /root/.local/share/mise/shims/node v24.19.0
  /root/work/sub /root/.local/share/mise/shims/node v24.19.0
```

The line, added after `'Subsystem sftp /usr/lib/openssh/sftp-server' \` in the `sshd_config.container` `printf`:

```
    'SetEnv PATH=/root/.local/share/mise/shims:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin' \
```

The path is a literal because that `RUN` precedes the `ENV MISE_DATA_DIR=…` line; it must stay equal to `MISE_DATA_DIR/shims`. The tail is Debian's root path, which `/etc/profile` also uses.

## The working-directory caveat

The shims resolve a version only where a mise config applies. The app keeps the only one at `/root/work/mise.toml` (`WORKSPACE_MOUNT`, `src-tauri/src/runtime/mod.rs`), and no global config exists, so from `$HOME` a shim fails with `mise ERROR No version is set for shim: node`. A client therefore has to work in `/root/work`, which is what Phase D's panel should tell the user to pick as the remote folder.

A global fallback was tried and works, but is *not* part of the recommendation: symlinking `/root/.config/mise/config.toml` to `/root/work/mise.toml` made node resolve from `/root` too (`mise config ls` in `/root/work` listed the file once). It would also send `mise use -g` into the workspace manifest, and its lockfile behaviour was not checked; it belongs in `04_improvements.md` if a client turns out to insist on `$HOME`.

## S4, headless part

`s4-client-death.ps1` opens `ssh <name> 'sleep 600'`, ends the client one of three ways, and polls `pgrep -a sshd` and `pgrep -a sleep` in the container plus the host's `podman.exe` ProxyCommand processes.

```
KillSsh:    [t+  5s] session open | container: 439 sshd: root@notty ; … 442 sleep 600 | host proxy podman.exe: 1
            [t+  0s] client ended | container: … 442 sleep 600                         | host proxy podman.exe: 0
            … unchanged through t+88s
KillProxy:  [t+  0s] client ended | container: … 498 sleep 600 (no sshd)               | host proxy podman.exe: 0
SuspendSsh: suspend status=0 ; ssh.exe thread wait reasons: Suspended=4
            [t+  0s] sshd: 609 sshd: root@notty   … still present at t+108s
            [t+123s] sshd: (none)
```

- **Killing `ssh.exe` ends the in-container `sshd` at once.** Windows OpenSSH takes its `ProxyCommand` child down with it, so the exec pipe closes and `sshd` reads end-of-file; the ClientAlive probe is not involved. Killing only the `podman.exe` proxy has the same effect.
- **A hung client is reaped by ClientAlive, but in about two minutes, not 45 seconds.** With `ssh.exe` suspended the pipe stays open; `sshd` logged `Timeout, client not responding from user root UNKNOWN port 65535` (debug log via a temporary `-E` in `mio-sshd`) and exited between 108 and 123 seconds after the suspend, in two runs. An earlier 85-second window saw it still alive.
- **The session's command outlives its `sshd`.** In every mode the `sleep 600` was reparented to PID 1 (`ps`: `498 1 sleep 600`) and kept running; a no-pty session's child gets no hangup. A client's remote agent that dies uncleanly can therefore leave its own process tree behind, which tini reaps only when it exits by itself. That, not a lingering `sshd`, is the residue S4's interactive half should look for.
