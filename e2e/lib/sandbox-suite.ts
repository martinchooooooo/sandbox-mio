import path from "path";

import ProjectsPage from "../test/pageobjects/projects.page.js";
import ProjectRow from "../test/pageobjects/project-row.component.js";
import { podman, LOCAL_CONNECTION } from "./podman-connection.js";

const WORKSPACE_ROOT = process.env.SANDBOX_WORKSPACE_ROOT;

// Every standard spec that drives a single throwaway sandbox copy-pasted the same three things: a
// `<prefix>-<timestamp>` project name, a `before()` asserting SANDBOX_WORKSPACE_ROOT is set (so a
// misconfigured run fails fast instead of producing a confusing ENOENT deep in a test body), and an
// `after()` best-effort `podman rm -f` so a failed run doesn't leave the container behind. Call this
// once per `describe` block. If a spec needs extra teardown (a remote rm -f, a forwardTerminate),
// register it in your own `after()` alongside this — Mocha runs `after()` hooks in registration
// order, and the cleanup here is a no-op-safe `{ check: false }` regardless of what runs before or
// after it.
export function useSandboxWorkspace(prefix: string): {
  projectName: string;
  row: ProjectRow;
  workspaceDir: string;
} {
  const projectName = `${prefix}-${Date.now()}`;
  const row = ProjectsPage.row(projectName);
  const workspaceDir = path.join(WORKSPACE_ROOT ?? "", projectName);

  before(() => {
    expect(WORKSPACE_ROOT).toBeTruthy();
  });

  after(() => {
    podman(LOCAL_CONNECTION, ["rm", "-f", projectName], { check: false });
  });

  return { projectName, row, workspaceDir };
}
