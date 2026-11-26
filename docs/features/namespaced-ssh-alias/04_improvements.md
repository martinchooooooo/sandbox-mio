---
type: improvements
title: "Namespaced SSH Alias — Deferred Improvements"
feature: namespaced-ssh-alias
---
# Namespaced SSH Alias — Deferred Improvements

Work identified while planning `docs/features/namespaced-ssh-alias/02_dev.md` that is out of its scope.

## Remove the orphaned bare-alias block of a pre-existing sandbox

A sandbox created before the namespaced alias keeps its bare `Host <name>` block in `~/.ssh/sandbox-mio/config`, which still shadows a same-named user host, and destroy removes only the `<name>.mio` block. Removing the bare block (`remove_alias` in `src-tauri/src/ssh/mod.rs`, called with the bare name) whenever the derived alias is written would end the residual shadowing. Deferred because the user chose to skip migration of pre-existing sandboxes; the cost is that clients configured with the bare alias would stop resolving it.

## Names Mutagen rejects as session names

`validate_name` (`src-tauri/crates/domain/src/lib.rs`) allows `_` and a leading digit, but Mutagen session names must start with a letter and hold only letters, digits and `-` (`docs/features/namespaced-ssh-alias/spike/01_dotted-alias-and-names.md`). A sandbox named `my_box` therefore cannot create its agent-web or expose-port forwards (`forward_name` in `src-tauri/src/network/mod.rs`), and one named `2fast` or `my_box` cannot deploy (the sync session is named with the bare sandbox name). Closing it needs a collision-free encoding of the name into session names, which would also let `NameHasDot` go. Deferred because it predates this feature and touches every forward and sync call site.

## Namespace the `Direct` alias

The `Direct` alias `deploy` writes (`src-tauri/src/backends/mod.rs`) is named after the user's Podman connection and can shadow a user `Host` entry of the same name. Renaming it breaks `resolve_machine` (`src-tauri/src/services/routing.rs`), which reads the connection back from the Mutagen sync session's beta host and uses it as the machine id. Closing it needs the machine id stored apart from the beta host token first. Deferred as a separate task because it touches the deploy and sync-session round trip.
