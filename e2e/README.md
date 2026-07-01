# End-2-End Testing

The suite is split into three tiers — **standard** (real podman machine, the default), **light**
(mocked, no podman, for edge cases and debugging), **heavy** (real machine + large assets) — so an
agent writing a new test knows exactly which tier it belongs to. See [`AGENTS.md`](./AGENTS.md) for
the taxonomy, decision rules, and the mock-backend design.

## Project structure

- `test/specs/standard/**` — the default WebdriverIO/Mocha UI tests (`specs` glob, `pnpm test`),
  against the real `mio-tests` machine. Write happy/common-path coverage here first.
- `test/specs/light/**` — mocked specs (`light` suite, `pnpm run test:light`), no podman. Reserved
  for edge cases that are awkward to provoke for real, and fast debug loops.
- `test/specs/heavy/**` — large-asset specs (`heavy` suite, `pnpm run test:heavy`).
- `test/pageobjects/` — the page-object model the specs use. One module per screen plus component objects; all selectors, waits, and clicks live here so specs read as high-level actions. See `test/pageobjects/AGENTS.md`.

## Test machines

Standard and heavy specs use two podman machines, both rootless and provisioned by `lib/podman-machine.ts`:

- `mio-tests` — the local machine the app talks to (passed to the app as `PODMAN_CONNECTION`).
- `mio-tests-remote` — an ephemeral stand-in for a remote deploy target, brought up for the deploy/sync tests and stopped afterwards unless `MIO_REMOTE_KEEP=1` (keep running) or `MIO_REMOTE_REMOVE=1` (delete its disk image).

Specs reach the machines directly with the `podman --connection <name>` CLI to assert on container state out-of-band from the UI.

## Page objects

The page-object model behind the specs in `../specs`. Every selector, wait, and click lives here
so specs read as high-level actions and a UI change touches one place.

### Layout

- `app.page.js` — base class the concrete pages extend.
- `*.page.js` — one module per screen (e.g. `greet.page.js`, `projects.page.js`). Each exports a
  singleton instance.
- `*.component.js` — a component object wrapping one repeated UI fragment rather than a whole
  screen (e.g. `project-row.component.js`, one project's `<li>` in the list). Constructed per
  instance, not a singleton.

### Conventions

- Expose selectors as getters and behaviours as `async` methods; specs never reference a raw
  selector.
- Resolve elements lazily on each access (re-query the root selector) so a component keeps working
  after the frontend re-renders it in place.
- Keep the waits with the action they guard — a method should return only once its effect is
  observable (e.g. `deployTo` resolves once the undeploy button is shown).
