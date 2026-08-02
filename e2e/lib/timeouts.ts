/**
 * Named wait budgets for WebDriver waits in page objects and specs.
 *
 * A budget names *what is being waited on*, not a duration — pick the one describing the slowest
 * real step in the wait and let the number follow. Two waits on the same kind of work must share a
 * budget, so a change lands on both (the deploy/undeploy pair is the reason this file exists).
 *
 * Raising a budget only makes a failing wait slower; lowering one can break a passing spec. When
 * collapsing several literals onto one name, take the largest of the group.
 *
 * Out of scope: mocha's own `this.timeout(...)` — that is a per-spec ceiling over many waits, not a
 * wait — and the host provisioning budgets in `podman-machine.ts`, which bound `spawnSync` calls
 * made before WebDriver exists.
 */

/** A render or route transition with no backend call behind it. */
export const UI_SETTLE = 5_000;

/** One Tauri command round-trip behind a click, and the DOM update that follows it. */
export const COMMAND = 10_000;

/** A region gated on an `onMount` that resolves several commands before its first element exists. */
export const PANEL_RENDER = 15_000;

/**
 * One external process step against the host or the VM: a podman container lifecycle call, a
 * Mutagen forward reaching or leaving `Connected`, a host-specs probe shelling out to PowerShell.
 */
export const SUBPROCESS = 30_000;

/** A Mutagen sync session settling on a steady status after a workspace scan and transfer. */
export const SYNC_CONVERGE = 60_000;

/**
 * Destroying a container that may hold a live foreground process: pane/forward teardown before
 * `podman rm --force` even runs, then the full SIGTERM grace for a process that may not exit clean.
 */
export const DESTROY_GRACE = 60_000;

/**
 * `podman machine start` plus rootless socket activation, driven through the app's recovery flow.
 * Covers machine start (≤30 s) + socket activation (≤7 s) + poll (4 × 0.5 s) + event delivery.
 */
export const PODMAN_RECOVERY = 90_000;

/** A deploy or undeploy end to end: image staging, SSH setup, Mutagen session start or teardown. */
export const DEPLOY = 120_000;

/** mise resolving and installing a package or runtime inside the container. */
export const PACKAGE_INSTALL = 180_000;

/** llama-server loading a model far enough to report a base_url. */
export const INFERENCE_START = 180_000;

/** Pulling a GGUF from HuggingFace. */
export const MODEL_DOWNLOAD = 480_000;

/** An agent's toolchain download and install inside the container. */
export const AGENT_INSTALL = 600_000;

/** The one-time base-image build on a connection that has never staged it. */
export const BASE_IMAGE_BUILD = 900_000;
