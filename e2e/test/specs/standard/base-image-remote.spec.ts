import ProjectsPage from "../../pageobjects/projects.page.js";
import { podman, podmanOk, LOCAL_CONNECTION, REMOTE_CONNECTION } from "../../../lib/podman-connection.js";
import { MISE_BASE_IMAGE } from "../../../lib/podman-machine.js";

// Regression guard for the missing base-image build on the remote deploy path. The mise base image
// is never published to a registry, so when it is absent on the target connection `podman create`
// falls back to a registry pull that fails ("requested access to the resource is denied"). The fix
// has `deploy` call `ensure_image_on` on the target connection before `podman create`; this spec
// removes the image from the remote and asserts a deploy still succeeds.
//
// Unlike base-image-build.spec.ts (which must sort first to run on a clean machine), this spec
// removes the image only from the remote connection and re-leaves the local connection untouched,
// so it has no ordering constraint with the rest of the suite.
describe("Base image bootstrap (remote)", () => {
  it("builds the mise base image on first remote deploy when it is absent on the target", async function () {
    // The base-image build on the remote downloads and bakes mise + zellij (~324 MB); generous
    // ceiling covers both the local create and the remote build.
    this.timeout(900000);

    // Force the bug's precondition: strip the image from the remote only.
    podman(REMOTE_CONNECTION, ["rmi", "--force", MISE_BASE_IMAGE], { check: false });
    expect(podmanOk(REMOTE_CONNECTION, ["image", "exists", MISE_BASE_IMAGE])).toBe(false);

    const projectName = `e2e-bootstrap-remote-${Date.now()}`;
    await ProjectsPage.open();
    const row = await ProjectsPage.createProject(projectName);

    // Against the unfixed backend, deploy fails here: `podman create` on the remote tries to pull
    // docker.io/sandbox-mio/base:mise and gets "requested access to the resource is denied".
    // The wide timeout covers the legitimate first-time build on the remote connection.
    await row.deployTo(REMOTE_CONNECTION, { timeout: 900000 });

    // The backend built the image on the remote — not pulled from a registry.
    expect(podmanOk(REMOTE_CONNECTION, ["image", "exists", MISE_BASE_IMAGE])).toBe(true);
    // The remote container is running, confirming create and start both succeeded.
    const state = podman(REMOTE_CONNECTION, [
      "inspect",
      projectName,
      "--format",
      "{{.State.Status}}",
    ]);
    expect(state).toBe("running");

    await row.destroy();
  });
});
