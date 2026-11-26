---
type: feature
title: "namespaced-ssh-alias"
description: "Give every sandbox an SSH alias that cannot collide with a real host (<name>.mio), replacing the refuse-the-name guard on the ssh-alias-shadow branch."
status: implemented
depends_on: [host-tool-ssh-access, podman-exec-transport]
architecture: [docs/architecture/transport.md]
---

# namespaced-ssh-alias

Give every sandbox an SSH alias that cannot collide with a real host (`<name>.mio`), replacing the refuse-the-name guard on the `ssh-alias-shadow` branch.

## Status note

Implemented, Phases A to E. Every sandbox's SSH alias is `<name>.mio`, derived in one function and used for every alias write, removal, forward and terminal host-key lookup; the `Host`-clash guard the branch `ssh-alias-shadow` carried is removed. `NameHasDot` stays because Mutagen session names reject dots. No migration: a sandbox created before the change keeps its bare-alias block (`04_improvements.md`). There is no `01_ideas.md`: the design was settled in conversation, and `02_dev.md` records the decisions.
