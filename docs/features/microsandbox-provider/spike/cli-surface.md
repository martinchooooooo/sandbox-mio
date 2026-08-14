# Spike: the `msb` CLI command and flag surface

This document records what microsandbox's `msb` CLI does and does not offer, measured against the primitives `MachineProvider` needs in `docs/features/microsandbox-provider/01_ideas.md`. Every answer is labelled **CONFIRMED** (documented at docs.microsandbox.dev), **CONTRADICTED** (the documentation or the upstream source states the opposite of what `01_ideas.md` assumes), or **UNKNOWN** (absent from the documentation).

Two evidence tiers appear below and are always named:

- **Documentation** — pages under `https://docs.microsandbox.dev/`, fetched 2026-08-09 from the index at `https://docs.microsandbox.dev/llms.txt`. Every CLI page in that index was read: `cli/overview.md`, `cli/sandbox-commands.md`, `cli/image-commands.md`, `cli/volume-commands.md`, `cli/ssh-commands.md`, plus `sandboxes/*`, `networking/overview.md`, `images/*`, and the seventeen `changelog/*` entries.
- **Upstream source** — the `superradcompany/microsandbox` repository at branch `main`, read 2026-08-09 via `https://raw.githubusercontent.com/superradcompany/microsandbox/main/<path>`. Source evidence resolves several questions the documentation leaves open, and it contradicts the documentation nowhere. It is still desk research: no `msb` binary has been run on the development host, so nothing here substitutes for spike stages 1 to 3 in `01_ideas.md`.

## Summary

Six of the ten questions are blockers for the design as written in `01_ideas.md`.

| # | Question | Verdict | Blocker | Effect on `01_ideas.md` |
|---|---|---|---|---|
| 1 | `msb create` / `msb run` flags | CONFIRMED | No | Every flag the design names exists, with the spellings recorded below |
| 2 | Ephemeral published host port | **CONTRADICTED** | **Yes** | The Validation claim that `-p` matches the `127.0.0.1::<port>` posture is wrong. `MsbProvider` must allocate host ports itself, and there is no `podman port` equivalent |
| 3 | `msb exec` flags | Partly CONTRADICTED | **Yes** | Piped stdin, `--workdir`, `--env`, and TTY all exist; **detached exec does not**. The paseo daemon start in `src-tauri/src/runtime/mod.rs` needs a different mechanism |
| 4 | Arbitrary labels | Partly CONTRADICTED | **Yes** | Labels are settable and filterable, but `msb ls --format json` does not return them. Recovering the `sandbox-mio.workspace` value costs one `msb inspect` per sandbox |
| 5 | `msb modify --port` | **CONTRADICTED** | **Yes** | `msb modify` has no port flag at all. The Open Question resolves negatively: the relay-pool plus in-container `socat` design stays for microsandbox |
| 6 | JSON output shapes | UNKNOWN in docs, CONFIRMED from source | **Yes** | Status strings are seven PascalCase values, not Podman's `created`/`running`/`exited`. `translateStatus()` and every file under `messages/` need new keys |
| 7 | Lifecycle event stream | **CONFIRMED ABSENT** | **Yes** | No such command exists. `listen_events` must poll or declare `event_stream: false` |
| 8 | `msb copy` | CONFIRMED syntax, CONTRADICTED for streams | No | Both directions work; neither end accepts stdin or stdout. Credential injection stays on piped `msb exec` |
| 9 | `msb load` / `msb save` | CONFIRMED | No | The `podman save` to `msb load` bridge is viable for the spike exactly as `01_ideas.md` proposes |
| 10 | Snapshots | CONFIRMED | No | The bootstrap-and-snapshot base-image path is fully supported, including snapshot labels |

The two questions that most change the design are 2 and 5 together: microsandbox can neither pick a free host port for the caller nor publish a new port after create, so `MsbProvider` inherits *both* halves of the Podman workaround (a pre-published relay pool at create time) *plus* a new obligation Podman does not impose (host-side port allocation before the create call).

## 1. `msb create` and `msb run` flag list

**CONFIRMED.**

Source URL: `https://docs.microsandbox.dev/cli/sandbox-commands.md` (sections "msb run" and "msb create"). The complete struct is `SandboxOpts` in `crates/cli/lib/commands/common.rs`, shared by both commands; `msb run` adds its own arguments in `crates/cli/lib/commands/run.rs`.

Exact spellings for every flag `01_ideas.md` asks about:

| Concern | Exact flag |
|---|---|
| name | `-n`, `--name` |
| cpus | `-c`, `--cpus` (plus `--max-cpus`) |
| memory | `-m`, `--memory` (plus `--max-memory`) |
| volume | `-v`, `--volume` |
| mount kinds | `--mount-dir`, `--mount-file`, `--mount-disk`, `--mount-named` |
| port | `-p`, `--port` |
| env | `-e`, `--env` |
| workdir | `-w`, `--workdir` |
| detach | `-d`, `--detach` (`msb run` only) |
| label | `--label` |
| replace | `--replace` (plus `--replace-with-timeout <DURATION>`) |

The image is a **positional argument**, not a flag. The documented invocations are:

```
msb run python -- python -c "print('hello')"
msb run --name devbox ubuntu -- bash
msb create python --name worker -c 2 -m 1G
msb create --replace python --name worker
```

Further flags present on both commands and relevant to the provider: `-t`/`--tty`, `--no-tty`, `-q`/`--quiet`, `--no-net`, `--net`, `--net-rule`, `--secret`, `--tmpfs`, `--copy`, `--copy-file`, `--copy-dir`, `--mkdir`, `--rm`, `--entrypoint`, `--init`, `--init-arg`, `--init-env`, `-H`/`--hostname`, `-u`/`--user`, `--pull`, `--root-disk`, `--shell`, `--script`, `--script-raw`, `--script-path`, `--security`, `--max-duration`, `--idle-timeout`, `--log-level`, and `--from-snapshot` (see question 10).

Four properties matter for `MsbProvider`:

- `msb create` "[t]akes the same flags as `msb run` (except `--detach`)" and boots the sandbox **without running a command**. This is closer to what `SandboxBackend::create` (`src-tauri/src/backends/mod.rs`) wants than `msb run` is.
- Without `--name` a sandbox is ephemeral and removed when the command finishes. `MsbProvider` must always pass `--name`.
- The CLI distinguishes a bind mount from a named volume "by looking for a `/` or `.` prefix" (`https://docs.microsandbox.dev/cli/volume-commands.md`). A Windows workspace path such as `C:\Users\...` has neither prefix, so `--volume` source inference is unsafe on Windows and `MsbProvider` should use the explicit `--mount-dir` form. Whether a drive-letter path parses at all is a spike stage 2 item.
- `--mount-named` creates a missing named volume idempotently and accepts `kind=dir|disk`, `size=...`, and `quota=...` in the option block.

## 2. Port syntax and ephemeral host ports

**CONTRADICTED.** The `127.0.0.1::<port>` form `create` (`src-tauri/src/backends/mod.rs`) relies on is rejected, and there is no ephemeral-port readback.

Source URLs: `https://docs.microsandbox.dev/cli/sandbox-commands.md`, `https://docs.microsandbox.dev/networking/overview.md`, and the changelog entry `https://docs.microsandbox.dev/changelog/2026-05-22.md` ("Configurable port bind addresses").

The documented forms are exactly these, and no other:

```
msb create python --name api -p 8080:80
msb run --name public-api -p 0.0.0.0:8000:8000 python
msb run nginx --port 0.0.0.0:8080:80 --port 127.0.0.1:9000:9000/udp
```

The networking page states the rule verbatim: "`-p 8080:80` and SDK helpers like `.port(8080, 80)` bind to `127.0.0.1`; `-p 127.0.0.1:8080:80` is the same local-only shape." The changelog states the grammar: "The CLI accepts `BIND_ADDR:HOST:GUEST` and `/udp` variants." A bracketed IPv6 bind address is also accepted, in the form `[ADDR]:HOST:GUEST[/udp]`.

**The Podman `127.0.0.1::8082` form fails.** Upstream source `crates/cli/lib/commands/common.rs`, function `parse_port_mapping`, splits the specification on `:` and accepts only two or three parts, then parses the host segment with a `u16` parse whose failure message is `invalid host port: {host_str}`. The three-part split of `127.0.0.1::8082` yields an empty host segment, which is not a valid `u16`, so the command errors before the sandbox is created. The only error messages the function can emit for a malformed specification are `invalid bind address: {bind_str}`, `invalid host port: {host_str}`, `invalid guest port: {guest_str}`, and `port must be in format HOST:GUEST[/udp] or BIND_ADDR:HOST:GUEST[/udp]`.

**`-p 0:8082` parses but is useless.** Zero is a valid `u16`, and `crates/network/lib/publisher.rs` binds the listener with a plain `TcpListener::bind(bind_addr)`, which the operating system would satisfy with an ephemeral port. But the assigned port is never observed: `publisher.rs` never calls `local_addr()` on the listener, and its `log_published_port_listener` helper logs the *requested* bind address. Nothing writes the assigned port back into the sandbox configuration, so no command can report it.

**There is no `podman port` equivalent.** The full top-level command list in `crates/cli/bin/main.rs` contains no port command. The nearest readback is the `ports` field of `msb ps --format json` (question 6), which `crates/cli/lib/commands/ps.rs` renders from the *stored configuration* as `"<host_bind>:<host_port>-><guest_port>/<protocol>"`. It echoes the request, so it can only ever report what the caller already passed.

Consequences for the design:

- `MsbProvider::create_instance` must choose concrete free host ports on the Windows host itself before building the argv, and must handle the race between choosing a port and `msb` binding it.
- The `--network pasta:--host-lo-to-ns-lo` equivalence question is separate and untouched by this finding: microsandbox's own documentation warns that "`loopback` means the sandbox's own `127.0.0.1`, not your laptop's localhost", so whether a guest server bound to guest loopback is reachable through a published port remains a spike stage 3 item.
- On Windows "the first published port may trigger a Windows Defender Firewall prompt for `msb.exe`" — a first-run interaction the welcome flow (`src/lib/welcome/WelcomeFlow.svelte`) has no counterpart for today.
- Publishing host ports is unavailable on microsandbox cloud, restating the rejection of the cloud target in `01_ideas.md`.

## 3. `msb exec` flags

**Partly CONTRADICTED.** Four of the five capabilities the app needs are present; detached execution is not.

Source URLs: `https://docs.microsandbox.dev/cli/sandbox-commands.md` (section "msb exec"), `https://docs.microsandbox.dev/sandboxes/commands.md`, and `https://docs.microsandbox.dev/changelog/2026-06-26.md`. Upstream source: `crates/cli/lib/commands/exec.rs`.

Documented flags: `-t`/`--tty`, `--no-tty`, `-e`/`--env`, `-w`/`--workdir`, `-u`/`--user`, `--timeout`, `--rlimit`, `-q`/`--quiet`. The upstream source adds one flag the CLI reference page omits: `--stream`, introduced in the 2026-06-26 changelog and described there as streaming "stdin and stdout bidirectionally with no PTY and no buffering".

```
msb exec devbox -- python -c "print('hello')"
msb exec worker -w /app -e PYTHONPATH=/app/lib --timeout 30s --rlimit nofile=1024 -- python compute.py
msb exec -t worker -e DEBUG=1 -w /app -- python
msb exec worker --stream -- node repl.js
```

Answers to the five capabilities:

- **(a) Piped stdin — CONFIRMED.** The documentation states: "The CLI auto-detects whether stdin is a terminal. When interactive, `msb exec` uses `attach` mode (TTY, line editing). When piped, it captures output." The upstream source reads piped stdin to EOF and forwards it to the guest, and `--stream` forwards it incrementally instead. This covers `tar_capture`/`tar_inject` (`src-tauri/src/credentials/mod.rs`) and the interactive agent auth session (`src-tauri/src/agents/mod.rs`), and means `ProviderCapabilities.piped_exec` can be true for microsandbox.
- **(b) Detached or background exec — CONTRADICTED.** There is no `-d`/`--detach` on `msb exec`; the flag exists only on `msb run`. The daemon starts in `PodmanMiseRuntime::prepare_terminal` and `prepare_agent_web` (`src-tauri/src/runtime/mod.rs`) therefore have no direct translation. The available substitutes, none verified: wrap the daemon in a shell that backgrounds it (`msb exec <name> -- sh -c 'nohup … &'`), register the daemon as a `--script` at create time and let `msb run -d` start it, or keep a long-lived `msb exec --stream` child process per daemon. The `msb exec` process-group change in `https://docs.microsandbox.dev/changelog/2026-07-05.md` — "`msb exec` now signals the full guest process group, so spawned background jobs do not survive as orphans" — is a direct warning that the `nohup` substitute may be killed when the exec returns.
- **(c) `--workdir` — CONFIRMED**, spelled `-w`, `--workdir`.
- **(d) `--env` — CONFIRMED**, spelled `-e`, `--env`, taking `KEY=VALUE`.
- **(e) TTY — CONFIRMED**, spelled `-t`, `--tty`, with `--no-tty` to force captured non-interactive execution. The two conflict.

One behavioural difference from `podman exec`: `msb exec` starts a stopped sandbox instead of failing. The upstream source routes through a `resolve_and_start` helper, and the documentation says the same of `msb copy`: it "connects to an existing sandbox and starts it temporarily if needed, following the same lifecycle ownership behavior as `msb exec`."

## 4. Labels

**Partly CONTRADICTED.** Labels exist and filter, but they are absent from list output.

Source URLs: `https://docs.microsandbox.dev/sandboxes/labels.md` and `https://docs.microsandbox.dev/cli/sandbox-commands.md`.

Create-time labels are CONFIRMED, repeatable, and arbitrary:

```
msb create python --name worker --label app=engine --label user.id=alice
msb modify worker --label tier=web --label-rm stale
msb ls --label app=engine
msb ps --label app=engine --label tier=web
```

Documented rules: values may be empty, so "[a] valueless label such as `--label gpu` is stored as `gpu=\"\"`, matching Docker's label semantics"; multiple `--label` selectors are AND-matched; the same selector drives `ps`, `ls`, `start`, `stop`, `restart`, `ping`, `touch`, and `rm`; and the image's own OCI labels are imported at create time with user labels winning on a key collision.

**Reserved key prefixes.** The documentation states that keys "cannot start with the reserved prefixes `sandbox.`, `microsandbox.`, or `service.`". Upstream source `sdk/rust/lib/sandbox/mod.rs` implements this as a literal `starts_with` over exactly those three strings. The app's chosen key `sandbox-mio.workspace` does not start with `sandbox.` (the character after `sandbox` is a hyphen), so **the key is legal as written** and needs no renaming.

**Labels are not returned by `msb ls`.** Upstream source `crates/cli/lib/commands/list.rs` emits four fields and none of them is labels; `crates/cli/lib/commands/ps.rs` likewise omits labels. Only `msb inspect --format json` carries them, nested at `config.spec.labels` as a string-to-string object (`crates/cli/lib/commands/inspect.rs` embeds the whole sandbox configuration under a `config` key, and the human-readable branch of the same file reads `config.spec.labels`).

Consequences: `MsbProvider::list_instances` cannot recover each sandbox's workspace path from one `msb ls` call. The options are one `msb inspect` per sandbox on every list (N+1 process spawns, against a list that `src/routes/projects/+page.svelte` refreshes often), or a `msb ls --label sandbox-mio.workspace=<path>` existence query when only one path is in question, or holding the mapping in the app's own state under `MIO_APP_DATA_DIR` and treating the label as recovery metadata only.

The label value is safe for a Windows path: upstream source `crates/cli/lib/ui.rs`, function `parse_label`, splits at the **first** `=` and takes the entire remainder as the value, so backslashes and the drive-letter colon in `C:\Users\krist\prog\example` pass through untouched. Whether a value that long survives the database and the metrics exporter is unverified; the labels page warns only about metric cardinality, and `msb-metrics --exclude-label-key <key>` exists to drop a noisy label from metric export while keeping it in the catalog.

## 5. `msb modify`

**CONTRADICTED for ports.** `msb modify` cannot add or change a published port, because it has no port flag.

Source URL: `https://docs.microsandbox.dev/cli/sandbox-commands.md` (section "msb modify"). Upstream source: `crates/cli/lib/commands/modify.rs`.

The complete documented flag list is `--cpus`, `--max-cpus`, `--memory`, `--max-memory`, `--env`, `--env-rm`, `--label`, `--label-rm`, `--workdir`, `--secret`, `--secret-rm`, `--dry-run`, `--next-start`, `--restart`, and `--format json`. The upstream argument struct matches exactly, adding only `--root-disk` and its hidden deprecated alias `--oci-upper-size`. There is no `--port`, no `--publish`, no `--volume`, and no `--mount` of any kind.

```
msb modify api --cpus 4
msb modify api --env MODE=prod --restart
msb modify api --max-memory 16G --next-start
msb modify api --cpus 8 --dry-run
```

`--restart` exists and is documented as "[r]estart if needed so restart-required changes become active now", and it conflicts with `--next-start`. But since no port flag exists, the question of whether adding a port would require `--restart` does not arise.

This resolves the first Open Question in `01_ideas.md` negatively. The relay-pool plus in-container `socat` design (`relay_pool_publish_args()` in `src-tauri/src/network/mod.rs`, `socat` in `src-tauri/resources/Containerfile`) remains necessary for microsandbox, exactly as it is for Podman, and `ProviderCapabilities.publish_after_create` is false for both providers. The only alternative is recreating the sandbox with `msb create --replace` (or `--replace-with-timeout`), which destroys the running zellij daemon and agent session and is therefore not an exposure mechanism.

Two further notes from the documentation that affect `MsbProvider`: "Env and workdir changes on a running sandbox apply to future execs only; running processes keep their current environment"; and `max_cpus`/`max_memory` "are boot-time reservations chosen at create time [that] default to the effective `cpus`/`memory`, which leaves no headroom", so `MsbProvider::create_instance` should set them above the initial values if live resize is ever wanted.

## 6. JSON output shapes

**UNKNOWN from the documentation; CONFIRMED from upstream source.**

The documentation shows `--format json` on `msb ls`, `msb ps`, `msb inspect`, `msb metrics`, `msb modify`, `msb images`, `msb image prune`, `msb volume ls`, and `msb snapshot ls`, but **prints no example output for any of them**. Every CLI page and every changelog entry was searched; no JSON document appears anywhere in the documentation for these commands. The field names below are therefore read from upstream source, not quoted from documentation.

```
msb ls --format json
msb ps --format json
msb ps -a --format json
msb inspect devbox --format json
```

**`msb ls --format json`** (`crates/cli/lib/commands/list.rs`, function `print_json`) emits a pretty-printed array whose every element has exactly four keys: `name`, `status`, `created_at`, `image`. No labels, no ports, no command, no resources. The `--running` and `--stopped` filters are applied client-side against the `Running` and `Stopped` statuses respectively.

**`msb ps --format json`** (`crates/cli/lib/commands/ps.rs`, function `status_json`) emits an array whose every element has exactly nine keys: `name`, `status`, `image`, `command`, `cpus`, `max_cpus`, `memory_mib`, `max_memory_mib`, `ports`. The `ports` value is an array of strings formatted `"<host_bind>:<host_port>-><guest_port>/<protocol>"`, for example `"127.0.0.1:8080->80/tcp"`, and is empty when networking is disabled or no port was published. The resource fields come from the *active* configuration for a running sandbox and the stored configuration otherwise, so an accepted `msb modify` shows immediately.

**`msb inspect --format json`** (`crates/cli/lib/commands/inspect.rs`) emits a single object with seven keys: `name`, `status`, `config`, `created_at`, `updated_at`, `active_config`, `pending_changes`. The `config` and `active_config` values are the full sandbox configuration document, which carries `spec.image`, `spec.labels`, `spec.network.ports` (with `host_bind`, `host_port`, `guest_port`, `protocol` per entry), `spec.resources`, `spec.env`, and `spec.mounts`. This is the only command that returns labels.

**Status values — the complete vocabulary.** All three commands render the status by debug-formatting the `SandboxStatus` enum, so the JSON strings are PascalCase. The enum is defined in `crates/db/lib/entity/sandbox.rs` with exactly seven variants, and the CLI's own `format_status` helper in `crates/cli/lib/ui.rs` matches the same seven strings when colouring the human-readable table:

| JSON value | Table value | Meaning per `https://docs.microsandbox.dev/sandboxes/lifecycle.md` and the upstream enum |
|---|---|---|
| `Created` | `created` | Created but not started. Documented in the enum as cloud-only today; local sandboxes go straight to `Running` |
| `Starting` | `starting` | Start submitted, not yet live. Also cloud-only today |
| `Running` | `running` | Guest agent ready |
| `Draining` | `draining` | Graceful shutdown in progress; new exec calls rejected |
| `Paused` | `paused` | Paused |
| `Stopped` | `stopped` | VM shut down, configuration persisted, restartable |
| `Crashed` | `crashed` | VM exited unexpectedly |

Note the case mismatch: the JSON carries `Running` while the human table prints `running`. `translateStatus()` (`src/lib/sandbox.ts`) must lowercase before lookup, and the `status_<value>` keys needed in every file under `messages/` are the seven lowercase forms. Podman's `exited` has no counterpart; the nearest microsandbox states are `Stopped` and `Crashed`, which Podman does not distinguish. Separately, `msb metrics` uses a *different* three-value vocabulary for its own `STATE` column — `running`, `stalled`, `exited` — which must not be confused with the sandbox status.

The `image` field is the OCI reference for an image-backed sandbox, the host path for a bind or disk-image rootfs, and the literal string `"-"` when the stored configuration cannot be parsed. `ScriptedMsbRunner` (`src-tauri/src/mockctl/mod.rs`) has enough here to emit faithful `msb ls` and `msb ps` documents.

One adjacent shape, for contrast only: microsandbox cloud's REST API at `https://docs.microsandbox.dev/api-reference/endpoints/sandboxes/list-sandboxes.md` returns a *different* document (`id`, `org_id`, `name`, `slug`, `status`, `ephemeral`, `created_at`, `spec`, …) with a *different* status enum (`created`, `starting`, `running`, `stopping`, `stopped`, `failed`, lowercase). That shape is not what the local CLI prints and must not be used as the parsing target.

## 7. Lifecycle events

**CONFIRMED ABSENT.** There is no equivalent of `podman events --format json`.

No events command appears on any documentation page, in any of the seventeen changelog entries, or in the top-level command enum in upstream source `crates/cli/bin/main.rs`. That enum is the complete list of `msb` subcommands: `run`, `create`, `modify`, `start`, `stop`, `restart`, `ping`, `touch`, `list` (alias `ls`), `status` (alias `ps`), `metrics`, `remove` (alias `rm`), `exec`, `copy` (alias `cp`), `logs`, `image`, `pull`, `load`, `save`, `registry`, `ssh`, `images`, `rmi`, `inspect`, `volume` (alias `vol`), `snapshot` (alias `snap`), `install`, `uninstall`, `doctor`, `update`, `downgrade`, `completion`, `context` (alias `ctx`), plus the hidden `sandbox`, `schema-baseline`, and `windows-self-swap` internals.

The two streaming surfaces that do exist, neither of which is a fleet-wide event stream:

```
msb logs devbox -f --json
msb metrics --follow --interval 1s
```

`msb logs` is per sandbox and emits JSON Lines. Under `--source system` it includes "synthetic lifecycle markers (`--- sandbox started ---` / `--- sandbox stopped ---`)", which is the closest thing to a lifecycle signal, but it requires one long-lived child process per sandbox and covers only start and stop. `msb metrics --follow` streams "one JSON Lines object per sandbox per interval" and its `--format json` output "add[s] `state` and `cpus` fields" — a fleet-wide poll in stream clothing, restricted to running, stalled, and recently exited sandboxes.

For `listen_events` (`src-tauri/src/backends/types.rs`) this leaves the three options `01_ideas.md` already lists, with polling `msb ls --format json` the most faithful and `ProviderCapabilities.event_stream: false` the most honest. Note that `listen_events` is one of the two flows already gated on `mock_backend_enabled()` because a streamed child cannot use the `ProcessRunner` seam, so a polling implementation would actually fit the seam *better* than Podman's does.

## 8. `msb copy`

**CONFIRMED for both directions; CONTRADICTED for streaming.**

Source URL: `https://docs.microsandbox.dev/cli/sandbox-commands.md` (section "msb copy"). Upstream source: `crates/cli/lib/commands/copy.rs`.

All four documented forms:

```
msb copy ./local.txt devbox:/tmp/local.txt
msb copy devbox:/tmp/out.txt ./out.txt
msb copy devbox:/tmp/a devbox:/tmp/b
msb copy devbox:/tmp/a otherbox:/tmp/a
```

The documentation states that "[h]ost-to-sandbox and sandbox-to-host forms follow `docker cp` syntax" and that "[s]ame-sandbox and cross-sandbox forms are microsandbox extensions". The short alias `msb cp` is available. The only flag is `-q`/`--quiet`.

**No stdin or stdout streaming.** The upstream argument struct is a positional `source`, a positional `destination`, and `--quiet`; both positionals are plain path strings, and no `-` convention is documented or implemented. This is unlike `docker cp`, which accepts `-` for a tar stream on either end.

Consequence for `src-tauri/src/credentials/mod.rs`: the `tar_capture` and `tar_inject` helpers that today pipe a tar stream through `podman exec -i` cannot switch to `msb copy`. They must either stay on piped `msb exec` (which does support stdin, per question 3) or stage the archive as a temporary host file and use `msb copy`. Piped `msb exec` is the closer match and avoids writing credential material to the host filesystem. `seed_paseo_projects` (`src-tauri/src/runtime/mod.rs`), which uses `podman cp` with real file paths, maps to `msb copy` directly.

## 9. `msb load` and `msb save`

**CONFIRMED**, including `--tag`.

Source URL: `https://docs.microsandbox.dev/cli/image-commands.md`. Corroborated by `https://docs.microsandbox.dev/changelog/2026-06-05.md`: "Both commands work over stdin or files (`--input` / `--output`); save defaults to Docker format and accepts `--format oci` for OCI Image Layout."

```
docker save my-image:latest | msb load
msb load --input my-image.tar
msb load --input oci-layout.tar --tag my-image:latest
msb save --output my-image.tar my-image:latest
msb save --format oci --output my-image.oci.tar my-image:latest
msb save my-image:latest > my-image.tar
```

`msb load` flags: `-i`/`--input <PATH>` ("Read archive from a tar file instead of stdin"), `-t`/`--tag <REF>` ("Add a local image reference to the first imported image"), `-q`/`--quiet`. The expanded form is `msb image load`. Both a Docker image archive and an OCI Image Layout archive are accepted, which is exactly what `podman save` produces.

`msb save` flags: `--format <FORMAT>` (`docker` or `oci`, default `docker`), `-o`/`--output <PATH>`, `-q`/`--quiet`. The expanded form is `msb image save`.

**How the loaded image is referenced afterwards.** The `--tag <REF>` value becomes a local image reference, used as the positional image argument on `msb create` or `msb run` — so `podman save sandbox-mio/base:7 | msb load --tag sandbox-mio/base:7` followed by `msb create sandbox-mio/base:7 --name <sandbox>` is the shape of the development bridge `01_ideas.md` proposes. Without `--tag`, the reference is whatever the archive carries. `msb images` (`msb image ls`) lists cached references and supports `--format json` and `-q`, and `msb image inspect <ref>` shows manifest, layers, and config.

One caveat the documentation calls out for the reverse direction: "`msb save` re-exports images from microsandbox's EROFS cache. The saved archive is semantically equivalent, but it is not a byte-for-byte copy of the originally pulled image. Manifest digest and layer digests can change because layer tar streams are regenerated." So an `msb save` output is not a digest-stable artefact and cannot be used for content-addressed comparison against the Podman-built image.

`msb image prune` "[n]ever removes images used by existing sandboxes or indexed snapshots", which protects a snapshot-based base image from a prune.

## 10. Snapshots

**CONFIRMED**, including snapshot labels.

Source URL: `https://docs.microsandbox.dev/sandboxes/snapshots.md`. Upstream source: `crates/cli/lib/commands/snapshot.rs`.

The full documented flow, which is precisely the bootstrap-and-snapshot base-image path in `01_ideas.md`:

```
msb run --name baseline --detach python:3.12
msb exec baseline -- pip install requests
msb stop baseline
msb snapshot create after-pip-install --from baseline
msb run --name worker --from-snapshot after-pip-install -- python -c "import requests; print(requests.__version__)"
```

**Create.** `msb snapshot create <NAME> --from <SANDBOX>`. Documented flags and their upstream counterparts: `--from <SANDBOX>` (required), `--dest-dir <DIR>` ("[t]he artifact lands at `DIR/<name>`"), `--label <K=V>` (repeatable), `-f`/`--force` (overwrite an existing artifact), `--integrity` (compute and record a content hash), `-q`/`--quiet`, and `--resumable`, which upstream source marks as reserved by the public contract and which "[c]urrent runtimes return an unsupported-feature error" for.

```
msb snapshot create after-pip-install --from baseline --label stage=ready
msb snapshot create after-pip-install --from baseline --dest-dir /mnt/big
msb snapshot create after-pip-install --from baseline --integrity
```

The running-sandbox rejection in `01_ideas.md` is confirmed and slightly wider than recorded there: "Snapshots are **disk-only** and require a sandbox that is not running. Stopped and crashed sandboxes can be snapshotted; running, draining, and paused sandboxes are rejected."

**List, inspect, remove, move.**

```
msb snapshot ls
msb snapshot ls --format json
msb snapshot inspect after-pip-install
msb snapshot inspect after-pip-install --verify
msb snapshot verify after-pip-install
msb snapshot rm after-pip-install
msb snapshot rm after-pip-install --force
msb snapshot reindex
msb snapshot save after-pip-install /tmp/snap.tar.zst --with-image
msb snapshot load /tmp/snap.tar.zst
```

`msb snapshot list` carries the visible alias `ls`, `remove` carries `rm`, and the whole group carries the top-level alias `msb snap`. `msb snapshot ls -q` shows digests only, not names. A snapshot is addressed "by name, digest, or path". `msb snapshot save` also takes `--with-parents` and `--plain-tar`; `msb snapshot load` takes an optional destination directory. The default location is `~/.microsandbox/snapshots/<name>/`, and "[t]hat whole directory is the snapshot" — there is no hidden daemon state, so `scp -r` of the directory is a documented transport.

**Boot from a snapshot.** `--from-snapshot <PATH_OR_NAME>` (upstream alias `--from-snap`) on `msb run`, and on `msb create` through the same shared options. It is mutually exclusive with the positional image argument, because "[a] snapshot already pins its image".

**Version or tag parsing.** There is no tag or version field on a snapshot. Two parseable carriers exist instead: the snapshot **name**, which is free-form and is the snapshot's identity, and snapshot **labels** via `--label K=V` at create time. Either supports the `BASE_IMAGE_VERSION` idiom — a name such as `sandbox-mio-base-7`, or a label such as `sandbox-mio.base-version=7`. Whether `msb snapshot ls --format json` returns labels is **UNKNOWN** and is a spike stage 3 item; if it does not, the name is the only cheap carrier and should be preferred.

This resolves part of the fourth Open Question in `01_ideas.md`. Note that `parse_image_version` (`src-tauri/src/backends/mod.rs`) cannot be reused unchanged for a snapshot-booted sandbox: because the snapshot pins the underlying image, the `image` field of `msb ls --format json` reports the pinned OCI reference (`docker.io/library/debian:12-slim`), not the snapshot name — inferred from the `extract_image` helper in `crates/cli/lib/commands/list.rs`, and unverified on a host. The version must therefore come from the sandbox's own `--label` at create time, read back through `msb inspect --format json`, rather than from the image string.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| `msb create -p 127.0.0.1::8082 …` fails with `invalid host port:` | `parse_port_mapping` (`crates/cli/lib/commands/common.rs`) splits on `:` and parses the host segment as a `u16`; the Podman ephemeral form leaves that segment empty | Have `MsbProvider` pick a concrete free host port on Windows before building the argv, and pass the full `127.0.0.1:<host>:<guest>` form |
| `-p 0:8082` is accepted but the sandbox is unreachable | Port zero binds an operating-system-chosen port, but microsandbox never reads `local_addr()` back, so `msb ps --format json` reports `127.0.0.1:0->8082/tcp` and the real port is unobservable | Never pass zero; there is no readback command, so an explicitly chosen host port is the only workable form |
| A user-exposed port cannot be published on a running microsandbox sandbox | `msb modify` has no port flag at all (`crates/cli/lib/commands/modify.rs`) | Keep the pre-published relay pool (`relay_pool_publish_args()` in `src-tauri/src/network/mod.rs`) and the in-container `socat` bridge for microsandbox, and set `ProviderCapabilities.publish_after_create` false |
| A `status_running` translation key is missing at runtime for a microsandbox sandbox | The JSON `status` is the PascalCase `Running`, while `translateStatus()` (`src/lib/sandbox.ts`) builds `status_<value>` keys from Podman's lowercase vocabulary | Lowercase the status at the provider boundary and add the seven keys `status_created`, `status_starting`, `status_running`, `status_draining`, `status_paused`, `status_stopped`, `status_crashed` to every file under `messages/` |
| The sandbox list renders without workspace paths even though every sandbox carries the label | `msb ls --format json` returns only `name`, `status`, `created_at`, `image` (`crates/cli/lib/commands/list.rs`); labels appear only under `config.spec.labels` in `msb inspect --format json` | Either pay one `msb inspect` per sandbox, or hold the name-to-workspace mapping in the app's own state and use the label for recovery and for `msb ls --label` queries only |
| A microsandbox sandbox's paseo or zellij daemon dies as soon as the starting exec returns | `msb exec` has no detached mode, and since the 2026-07-05 release it "signals the full guest process group, so spawned background jobs do not survive as orphans" | Register the daemon as a `--script` at `msb create` time, or supervise a long-lived `msb exec --stream` child, rather than backgrounding inside a one-shot exec |
| Credential capture or injection cannot be ported to `msb copy` | `msb copy` takes two plain path strings and no `-` stdin or stdout convention (`crates/cli/lib/commands/copy.rs`) | Keep the tar stream on piped `msb exec`, which does read stdin to EOF, instead of moving it to `msb copy` |
| `msb create -v C:\Users\…\project:/root/work` mounts a named volume instead of the host directory | The CLI "distinguishes bind mounts from named volumes by looking for a `/` or `.` prefix"; a Windows drive-letter path has neither | Use the explicit `--mount-dir <SOURCE>:<DEST>[:OPTIONS]` form for every workspace mount |
| Automating an event feed against `msb events` fails because the command does not exist | No events command appears in the documentation or in the top-level command enum (`crates/cli/bin/main.rs`) | Poll `msb ls --format json`, or declare `ProviderCapabilities.event_stream` false and leave the activity feed quieter for microsandbox sandboxes |
