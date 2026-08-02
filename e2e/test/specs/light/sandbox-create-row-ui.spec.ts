import assert from "node:assert";

import ProjectsPage from "../../pageobjects/projects.page.js";

// LIGHT spec for docs/features/sandbox-list-create: runs against MIO_MOCK_BACKEND=1, no podman.
// Covers the always-present create row on an empty list; the real bind-mount path (existing_path
// against a real directory) is standard-tier coverage instead.
//
// TODO: the other scoped scenario — client-side name-collision pre-check — isn't covered here.
// Submitting the same name twice reliably creates two mock sandboxes despite CreateSandboxItem's
// `projects.some(...)` guard looking correct by inspection. Ruled out: a stale `projects` prop, an
// out-of-order `loadProjects()` response, and a text-selector false-positive from the guard's own
// error message. See docs/features/sandbox-list-create/02_dev.md Phase D notes before re-investigating.

describe("Sandbox create row — UI sanity (mock backend)", function () {
  this.timeout(30000);

  it("shows the create row and the folder-picker link even with zero sandboxes", async () => {
    await ProjectsPage.goHome();
    await ProjectsPage.open();
    await ProjectsPage.ensurePodmanReady();

    assert(
      await (await ProjectsPage.createRow).isDisplayed(),
      "The create row did not render on an empty sandbox list — the {:else} restructuring " +
        "(docs/features/sandbox-list-create/02_dev.md) must render the <ul>/create row " +
        "independent of projects.length.",
    );
    assert(
      await (await ProjectsPage.useExistingFolderLink).isDisplayed(),
      "The 'use an existing folder' link is missing from the create row.",
    );

    await ProjectsPage.useExistingFolderMode();
    assert(
      await (await ProjectsPage.chooseFolderButton).isDisplayed(),
      "Clicking the folder-picker link did not reveal the 'Choose folder…' button — the " +
        "mode toggle in CreateSandboxItem.svelte did not switch to 'folder' mode.",
    );
    assert(
      !(await (await ProjectsPage.nameInput).isExisting()),
      "The name input is still present after switching to 'folder' mode — the two modes must " +
        "be mutually exclusive.",
    );

    await (await ProjectsPage.backToNameButton).click();
    assert(
      await (await ProjectsPage.nameInput).isDisplayed(),
      "The name input did not come back after clicking the 'Back' control.",
    );
  });
});
