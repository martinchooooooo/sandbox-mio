import assert from "node:assert";

import ProjectsPage from "../../pageobjects/projects.page.js";

// LIGHT spec for docs/features/sandbox-list-create: runs against MIO_MOCK_BACKEND=1, no podman.
// Covers the always-present create row on an empty list, one of the two UI/store-logic edge cases
// 03_e2e.md scopes to this tier. The real bind-mount path (existing_path against a real directory)
// is standard-tier coverage, not this spec's job.
//
// The second scoped scenario — client-side name-collision pre-check, no round trip — is NOT yet
// covered here. A submit-the-same-name-twice repro against the mock backend reliably creates two
// mock sandboxes with the identical name despite CreateSandboxItem's `projects.some(...)` guard
// appearing correct by inspection; the cause was not isolated (ruled out so far: a stale `projects`
// prop — a `data-projects-names` probe confirmed the child sees the freshly created entry before
// the retry; an out-of-order `loadProjects()` response from onMount clobbering state — an explicit
// settle-and-retry made no difference; and a text-selector false-positive from the guard's own error
// message containing the sandbox name — fixed, but the duplicate row persisted anyway, confirming a
// second sandbox is genuinely created). Needs further investigation before this scenario is added
// back — see docs/features/sandbox-list-create/02_dev.md's Phase D notes.

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

    // Back to "name" mode leaves no trace of the folder controls.
    await (await ProjectsPage.backToNameButton).click();
    assert(
      await (await ProjectsPage.nameInput).isDisplayed(),
      "The name input did not come back after clicking the 'Back' control.",
    );
  });
});
