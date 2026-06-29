# Activity Center spikes

Throwaway scripts kept as evidence for the decisions recorded in `../01_ideas.md` (Validation
section). Not part of the build; nothing here is wired into the app or CI. Re-runnable for
reference.

## `race_spike.rs` — hydration-vs-stream race

Models the open question "between the `list_tasks` snapshot read and the
`listen("activity://task-updated")` registration, an emit can be missed or applied out of order".
It walks one task through five monotonically version-stamped updates and brute-forces every
interleaving of (snapshot point, subscribe point) and every delivery order of buffered live
events, comparing two consumer protocols against the producer's final truth.

Run (Windows, `rustc` on PATH):

```
rustc -O race_spike.rs -o race_spike.exe && ./race_spike.exe
```

Result observed 2026-06-29:

- **Protocol A** (snapshot-then-subscribe, no version fold): 5 of 21 racy cuts lost the terminal
  update — confirmed bug.
- **Protocol B** (subscribe-then-snapshot, fold by version): all 873 cut × delivery-order
  combinations converged — confirmed safe and order-independent.

Conclusion carried into the design: the consumer must register the subscription **before**
reading the snapshot **and** guard every fold (snapshot entries included) with a monotonic
per-task version. Neither measure alone suffices.

## Transport API-surface check (no script)

The other 2026-06-29 spike was a `grep` over the installed `@tauri-apps/api@2` type declarations
to confirm the `emit`/`listen` and `Channel`/`invoke` signatures the design depends on. It left
no script — reproduce with:

```
grep -nE 'declare function (listen|emit)|type UnlistenFn' node_modules/@tauri-apps/api/event.d.ts
grep -nE 'declare (function invoke|class Channel)' node_modules/@tauri-apps/api/core.d.ts
```
