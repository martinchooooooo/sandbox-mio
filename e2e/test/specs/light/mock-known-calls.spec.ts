import assert from "node:assert";

import { configureMock, configureMockExpectingError, useMockControl } from "../../../lib/mock.js";

// LIGHT spec proving the mock control plane's own key validation
// (docs/features/scriptable-mock-backend/02_dev.md, Resolved Decision 1 and 9). This feature has no
// Rust #[test] fns; mockctl's correctness is proven here via the real mock_configure IPC round-trip.

// Must stay equal to KNOWN_CALLS in src-tauri/src/mockctl/mod.rs. A key added there but missed here
// is a scriptable call with no coverage; a key here but not there fails this spec loudly.
const KNOWN_CALLS = [
  "podman.create",
  "podman.ps",
  "podman.start",
  "podman.stop",
  "podman.rm",
  "podman.machine-list",
  "podman.machine-start",
  "podman.system-connection-list",
  "podman.image-exists",
  "podman.build",
  "podman.info",
  "mutagen.sync-create",
  "mutagen.sync-pause",
  "mutagen.sync-resume",
  "mutagen.sync-terminate",
  "mutagen.sync-flush",
  "mutagen.sync-list",
];

describe("Mock control plane — KNOWN_CALLS validation (mock backend)", function () {
  this.timeout(60000);

  useMockControl();

  it("accepts every known key", async () => {
    for (const key of KNOWN_CALLS) {
      await configureMock(key, { kind: "return-value", stdout: "" });
    }
  });

  it("rejects an unknown key with a MockControl error", async () => {
    const err = await configureMockExpectingError("podman.not-a-real-subcommand", {
      kind: "return-value",
      stdout: "",
    });
    assert(err, "mock_configure resolved for an unknown key — KNOWN_CALLS is not being enforced");
    assert.strictEqual(err.kind, "MockControl");
  });

  it("rejects a fail behaviour that would exit zero", async () => {
    const err = await configureMockExpectingError("podman.start", {
      kind: "fail-always",
      exit_code: 0,
      stderr: "would never fail",
    });
    assert(
      err,
      "mock_configure accepted a fail behaviour with exit_code 0 — check() would treat it as success",
    );
    assert.strictEqual(err.kind, "MockControl");
  });
});
