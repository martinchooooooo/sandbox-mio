import assert from "node:assert";

import ProjectsPage from "../../pageobjects/projects.page.js";

// LIGHT spec for docs/features/microsandbox-provider (Phase J). Runs against MIO_MOCK_BACKEND=1, so
// the `msb.*` calls land on ScriptedMsbRunner's tracked-instance round trip rather than a real `msb`
// binary — this is the first spec that drives MsbProvider at all, since nothing reached it before
// the create-flow picker existed (02_dev.md, Phase G's verify note).
//
// The two assertions are the pair Phase J is really about: a sandbox created on microsandbox comes
// back tagged as such through `list_sandboxes`, and its deploy action is gated off by
// `ProviderCapabilities.remote_targets` while a Podman sandbox in the same list keeps it.
describe("Microsandbox provider — create and capability gating (mock backend)", function () {
  this.timeout(60000);

  const msbName = `e2e-msb-${Date.now()}`;
  const podmanName = `e2e-podman-${Date.now()}`;

  it("creates a sandbox on the microsandbox provider with no deploy action", async () => {
    await ProjectsPage.goHome();
    await ProjectsPage.open();
    await ProjectsPage.ensurePodmanReady();

    await ProjectsPage.chooseProvider("microsandbox");
    const row = await ProjectsPage.createProject(msbName);

    assert(
      !(await (await row.deployButton).isExisting()),
      "A microsandbox sandbox still offers Deploy — SandboxItem.svelte must hide it when the " +
        "provider's ProviderCapabilities.remote_targets is false.",
    );
  });

  it("keeps the deploy action for a Podman sandbox in the same list", async () => {
    await ProjectsPage.chooseProvider("podman");
    const row = await ProjectsPage.createProject(podmanName);

    assert(
      await (await row.deployButton).isDisplayed(),
      "A Podman sandbox lost its Deploy action — the capability gate must be per sandbox, not " +
        "applied to the whole list.",
    );
  });

  it("destroys the microsandbox sandbox", async () => {
    await ProjectsPage.row(msbName).destroy();
    await ProjectsPage.row(podmanName).destroy();
  });
});
