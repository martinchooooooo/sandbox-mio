# Spike Artefacts — Podman Process Isolation

Throwaway Windows probes written on 2026-08-16/17 to locate why every `podman` process spawned by
`sandbox-mio.exe` hung forever while the identical binary and argv returned in under a second when
run from a shell. Kept as the evidence trail behind `../01_ideas.md`. None of the probes are part of
the app build; each is a single file compiled on demand.

Build any probe with `rustc --edition 2021 -O <file>.rs -o <file>.exe`. The two GUI-subsystem probes
(`spawn-from-gui-parent.rs`, `spawn-from-gui-child.rs`) have no console output and take a log file
path as their first argument.

## Diagnostic probes

`thread-stack-sample.rs` takes a process id and a thread id, suspends the thread, reads RIP and RSP
via `GetThreadContext`, and names every stack address by nearest export in the system DLLs. System
DLLs load at the same base address in every process, so the probe's own export tables resolve the
target's addresses without symbols.

`handle-dump.rs` takes a process id, snapshots the system handle table via
`NtQuerySystemInformation(SystemExtendedHandleInformation)`, duplicates each handle belonging to the
target, and names the `File` objects. Each name query runs on a watchdog thread with an 800ms
deadline, so a handle whose `NtQueryObject` call blocks is reported as `<NAME QUERY HUNG>` instead of
hanging the probe. A hung `NtQueryObject(ObjectNameInformation)` is the signature of a synchronous
handle carrying a pending I/O that never completes.

`env-block-dump.rs` takes a process id and prints the target's environment block by walking
PEB → RTL_USER_PROCESS_PARAMETERS → Environment with `ReadProcessMemory`. Used to rule out a stray
`CONTAINERS_*` or `PODMAN_*` variable in the app's inherited environment.

## Spawn-shape probes

Each spawn-shape probe runs `podman machine list --format json` against the staged binary at
`src-tauri/target/debug/podman/podman.exe`, redirecting stdout and stderr to temp files, and reports
the wall time or `HUNG` past its deadline. Together the probes vary parent subsystem, console
attachment, stdin disposition, and creation flags.

`spawn-from-gui-parent.rs` is a GUI-subsystem parent covering four combinations of
`CREATE_NO_WINDOW`, inherited versus null stdin, and `CONTAINERS_HELPER_BINARY_DIR`.

`spawn-from-console-parent.rs` is a console-subsystem parent that is genuinely attached to the
launching console. The probe prints `GetConsoleWindow` and `GetFileType(stdin)` first, so the log
records that stdin really was a console handle (`FILE_TYPE_CHAR`, value 2) rather than a pipe.

`spawn-from-gui-child.rs` plus `console-launcher.rs` reproduce the app's exact topology: a console
process spawns a GUI-subsystem child with inherited stdio, and the GUI child spawns podman. A
GUI-subsystem image is never attached to the launching console, yet still inherits the console's raw
`\Device\ConDrv` handles — the same position `sandbox-mio.exe` occupies under `build.exe dev`.

`detached-process-check.rs` confirms podman behaves correctly when spawned with `DETACHED_PROCESS`
rather than `CREATE_NO_WINDOW`, which is the fix candidate `../01_ideas.md` proposes.

## Inheritance-control probes

Probes added on 2026-08-17 to choose between the inheritance-control mechanisms weighed in
`../01_ideas.md` Open Question 1.

`std-handle-list-check.rs` attempts `CommandExt::raw_attribute` with
`PROC_THREAD_ATTRIBUTE_HANDLE_LIST`, the only mechanism that restricts a child to an explicit set of
inherited handles. The probe does not compile on rustc 1.96.0 stable: `raw_attribute` remains behind
the unstable `windows_process_extensions_raw_attribute` feature. Keep the probe as the check to
re-run when the toolchain advances.

`handle-sanitise-check.rs` clears `HANDLE_FLAG_INHERIT` on every inheritable handle the process owns,
locating the handles with the same system handle snapshot `handle-dump.rs` uses, then spawns podman
before and after clearing to prove the clearing does not break stdio. `sanitize_lib.rs` holds the
clearing routine, shared by `inheritance-ab-test.rs` via `include!`.

`inheritance-ab-test.rs` spawns one long-lived child before clearing and one after, printing both
process ids so `handle-dump.rs` can compare what each child inherited.

## Captured results

All spawn-shape probes completed in 0.24s–1.72s and returned the full 584-byte machine list, in
every combination tried, including inherited console stdin under `CREATE_NO_WINDOW`. No synthetic
probe reproduced the hang, which is what pins the trigger on the poisoned console handle present in
the developer's terminal session rather than on the spawn shape alone.

`detached-process-check.rs` returned `exit code: 0 in 576.1211ms, 584 bytes, contains machine name:
true` under `DETACHED_PROCESS`, and `exit code: 0 in 244.6879ms` under `CREATE_NO_WINDOW`.

`thread-stack-sample.rs` against a hung podman process reported RIP at `ntdll.dll!
ZwQueryInformationFile+0x14`, a stack roughly 2KB deep whose only resolvable return addresses were
`kernel32.dll!BaseThreadInitThunk` and `ntdll.dll!RtlUserThreadStart`, and a loaded-module list
limited to ntdll, kernel32, KERNELBASE, ucrtbase, bcryptprimitives, powrprof, RPCRT4 and UMPDC. The
process was therefore blocked inside runtime initialisation, before podman parsed its arguments.

`handle-sanitise-check.rs` reported `10 inheritable handles found, 10 cleared` in a trivial console
process, and podman completed normally both before and after the clearing — so a process owns far
more inheritable handles than the three standard ones, and clearing the inherit flag does not
interfere with `std::process::Command` binding stdio at spawn time.

`inheritance-ab-test.rs` produced the result that settles the mechanism choice. The child spawned
**before** clearing inherited a `\Device\ConDrv` handle (value `0x778`) **despite** being spawned with
`DETACHED_PROCESS` and with all three standard handles bound explicitly. The child spawned **after**
clearing inherited zero `\Device\ConDrv` handles. Creation flags and explicit stdio therefore do not
prevent console-handle inheritance; only clearing the inherit flag does.

`handle-dump.rs` found the decisive evidence: the same wedged console handle values appear in
`build.exe`, in `sandbox-mio.exe`, and in every podman child. Handle `0x64` reports
`<NAME QUERY HUNG>` in all three processes, alongside several healthy `\Device\ConDrv` handles.
Inherited handles keep their numeric value, so the identical values prove inheritance rather than
coincidence.
