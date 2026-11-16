---
type: improvements
title: "Native Terminal Improvements"
feature: native-terminal
---
# Native Terminal Improvements

This document records work deferred out of the native terminal's scope, as planned in [02_dev.md](02_dev.md). Each section names the improvement and why it was deferred.

## Pre-warm the SSH session on sandbox selection

Opening the terminal's SSH session when a sandbox is selected, before its Terminal tab is clicked, would hide the engine exec and key exchange (a median 380 ms on Podman in the spike) and leave only the zellij attach cost of about 250 ms. Deferred by Resolved Decision 5 in `docs/features/native-terminal/02_dev.md`: pre-warming reverses the lazy-terminal behaviour `e2e/test/specs/standard/embedded-workspace.spec.ts` asserts, and the warm path may already feel fast enough once measured in the app.

## Skip the session-creating exec when the zellij session is known to run

`SshTerminalHost` (`src-tauri/src/terminal/mod.rs`) runs `zellij attach --create-background <name>` through the engine exec on every attach, concurrently with the SSH handshake (Resolved Decision 7). When the session is already running, the exec changes nothing but still costs one engine exec of Podman load. A reattach within one app run could skip it. Deferred because the exec overlaps the handshake, so it adds little latency, and skipping it safely needs a reliable signal that the session is running, which the spike did not record.

## Send terminal input as a raw IPC body

`terminal_input` (`src-tauri/src/commands/terminal.rs`) carries input as a JSON string. A large paste is therefore escaped into JSON, and raw X10 mouse bytes from wterm's `onBinary` callback cannot be sent at all. Tauri accepts a raw request body with the sandbox name in a header, which would carry arbitrary bytes. Deferred because typed input and pastes are small, and zellij enables SGR mouse reporting, which wterm delivers through `onData` as text.
