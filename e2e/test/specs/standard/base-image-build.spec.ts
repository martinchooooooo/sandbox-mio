import ProjectsPage from "../../pageobjects/projects.page.js";
import { podman, podmanOk, LOCAL_CONNECTION } from "../../../lib/podman-connection.js";
import { MISE_BASE_IMAGE } from "../../../lib/podman-machine.js";
import { captureDiagOnFailure } from "../../../lib/diag.js";

// Regression guard: the mise base image is never published to a registry, so when it is absent
// `podman create --pull=missing` falls back to a failing registry pull. create_sandbox must instead
// build it from the bundled Containerfile on first use.
//
// Sorted to run first: removing the image force-removes containers built on it, so this needs an
// otherwise-clean machine. The rebuilt ~324 MB image is left in place for later specs.
describe("Base image bootstrap", () => {
  captureDiagOnFailure();

  it("builds the mise base image on first sandbox create when it is absent", async function () {
    // The base-image build downloads and bakes mise + zellij (~324 MB), longer than the suite default.
    this.timeout(900000);

    // Force the bug's precondition: no base image staged on the connection.
    podman(LOCAL_CONNECTION, ["rmi", "--force", MISE_BASE_IMAGE], { check: false });
    expect(podmanOk(LOCAL_CONNECTION, ["image", "exists", MISE_BASE_IMAGE])).toBe(false);

    const projectName = `e2e-bootstrap-${Date.now()}`;
    await ProjectsPage.open();
    // Against the unfixed backend the create command rejects here (registry pull denied) and the
    // row never appears, failing the wait. The wide timeout covers the legitimate one-time build.
    const row = await ProjectsPage.createProject(projectName, { timeout: 900000 });

    // The app built the image as part of create...
    expect(podmanOk(LOCAL_CONNECTION, ["image", "exists", MISE_BASE_IMAGE])).toBe(true);
    // ...and the container exists on the connection, created from it.
    expect(podmanOk(LOCAL_CONNECTION, ["container", "exists", projectName])).toBe(true);

    await row.destroy();
  });
});
