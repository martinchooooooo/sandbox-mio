import assert from "node:assert";

import { invokeCmd } from "../../../lib/mock.js";

// The GitHub token is a single-value credential: write-only over IPC, listed by the generic bundle
// commands, removed by the generic delete (docs/features/techdebt-sweep/02_dev.md, Phase H).
// Command-level only, like standard/base-image-recipe.spec.ts: no page is driven, so no
// captureDiagOnFailure(). The credential store lives under the run's isolated app-data root.

const SENTINEL = "ghp_light_spec_sentinel_value";

type Bundles = [string, { capturedAtMs: number; accountEmail: string | null }][];

describe("GitHub token credential", function () {
  this.timeout(60000);

  after(async () => {
    await invokeCmd("delete_credential_bundle", { serviceId: "github" });
  });

  it("saves without echoing the value, lists as a bundle, and deletes", async () => {
    const manifest = await invokeCmd<{ capturedAtMs: number }>("set_github_token", { token: ` ${SENTINEL} ` });
    assert(manifest.capturedAtMs > 0, "set_github_token returned no saved date.");
    assert(!JSON.stringify(manifest).includes(SENTINEL), "set_github_token echoed the token value over IPC.");

    const listed = await invokeCmd<Bundles>("list_credential_bundles");
    assert(!JSON.stringify(listed).includes(SENTINEL), "list_credential_bundles leaked the token value over IPC.");
    assert(listed.some(([id]) => id === "github"), "The github bundle is missing from list_credential_bundles.");

    await invokeCmd("delete_credential_bundle", { serviceId: "github" });
    const after = await invokeCmd<Bundles>("list_credential_bundles");
    assert(!after.some(([id]) => id === "github"), "delete_credential_bundle did not remove the github bundle.");
  });

  it("refuses an empty or multi-line token", async () => {
    for (const token of ["   ", "a\nb"]) {
      await assert.rejects(invokeCmd("set_github_token", { token }), `token ${JSON.stringify(token)} was accepted.`);
    }
    const listed = await invokeCmd<Bundles>("list_credential_bundles");
    assert(!listed.some(([id]) => id === "github"), "A refused token still left a github bundle behind.");
  });
});
