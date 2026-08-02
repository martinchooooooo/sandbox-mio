import path from "path";

import ProjectsPage from "../test/pageobjects/projects.page.js";
import ProjectRow from "../test/pageobjects/project-row.component.js";
import { podman, LOCAL_CONNECTION } from "./podman-connection.js";

const WORKSPACE_ROOT = process.env.SANDBOX_WORKSPACE_ROOT;

/**
 * Names, guards and tears down one throwaway sandbox; call once per `describe`. Extra teardown
 * belongs in the spec's own `after()` — this one is `{ check: false }` and order-independent.
 */
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
