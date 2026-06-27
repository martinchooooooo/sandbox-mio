import ProjectsPage from "../pageobjects/projects.page.js";
import { podman, podmanOk, LOCAL_CONNECTION } from "../../lib/podman-connection.js";
import { FLOX_BASE_IMAGE } from "../../lib/podman-machine.js";

// Regression guard for the missing base-image build. The flox base image is never published to a
// registry, so when it is absent `podman create --pull=missing` falls back to a registry pull that
// fails ("requested access to the resource is denied"). The fix has `create_sandbox` build the
// image from the bundled Containerfile on first use; this spec removes the image and asserts a
// sandbox can still be created, which fails against the unfixed backend.
//
// This file is named to sort first so it runs before any other spec creates a container: removing
// the image force-removes containers built on it, so it must execute on an otherwise clean machine.
// Removing and rebuilding the ~1.27 GB image is the unavoidable cost of exercising the build path —
// the rebuilt image is left in place for the specs that follow.
describe("Base image bootstrap", () => {
  it("builds the flox base image on first sandbox create when it is absent", async function () {
    // The base-image build downloads and bakes flox (~1.27 GB), far longer than the suite default.
    this.timeout(900000);

    // Force the bug's precondition: no base image staged on the connection.
    podman(LOCAL_CONNECTION, ["rmi", "--force", FLOX_BASE_IMAGE], { check: false });
    expect(podmanOk(LOCAL_CONNECTION, ["image", "exists", FLOX_BASE_IMAGE])).toBe(false);

    const projectName = `e2e-bootstrap-${Date.now()}`;
    await ProjectsPage.open();
    // Against the unfixed backend the create command rejects here (registry pull denied) and the
    // row never appears, failing the wait. The wide timeout covers the legitimate one-time build.
    const row = await ProjectsPage.createProject(projectName, { timeout: 900000 });

    // The app built the image as part of create...
    expect(podmanOk(LOCAL_CONNECTION, ["image", "exists", FLOX_BASE_IMAGE])).toBe(true);
    // ...and the container exists on the connection, created from it.
    expect(podmanOk(LOCAL_CONNECTION, ["container", "exists", projectName])).toBe(true);

    await row.destroy();
  });
});
