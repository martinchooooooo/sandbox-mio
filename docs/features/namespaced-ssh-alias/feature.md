---
type: feature
title: "namespaced-ssh-alias"
description: "Give every sandbox an SSH alias that cannot collide with a real host (<name>.mio), replacing the refuse-the-name guard on the ssh-alias-shadow branch."
status: in-progress
depends_on: [host-tool-ssh-access, podman-exec-transport]
architecture: [docs/architecture/transport.md]
---

# namespaced-ssh-alias

Give every sandbox an SSH alias that cannot collide with a real host (`<name>.mio`), replacing the refuse-the-name guard on the `ssh-alias-shadow` branch.

## Status note

In progress. Phases A and B implemented: the `.mio` suffix is confirmed, `NameHasDot` stays (Mutagen rejects dotted session names), and every alias write, removal, forward and terminal lookup derives the alias from the name. Phases C to E remaining. The branch `ssh-alias-shadow` carries the guard this feature replaces and must not merge before Phase C of `02_dev.md` removes it. There is no `01_ideas.md`: the design was settled in conversation, and `02_dev.md` records the decisions.
