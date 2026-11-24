import path from "path";

import ProjectsPage from "../test/pageobjects/projects.page.js";
import ProjectRow from "../test/pageobjects/project-row.component.js";
import { podman, LOCAL_CONNECTION } from "./podman-connection.js";

const WORKSPACE_ROOT = process.env.SANDBOX_WORKSPACE_ROOT;

/**
 * Force-removes a throwaway sandbox with its proxy sidecar and network, ignoring what is absent.
 * Every sandbox owns a proxy sidecar and its own `mio-proxy-<name>` network (src-tauri/src/proxy);
 * removing only the sandbox leaks that network, and each leaked one costs the next create about a
 * second while it probes for a free subnet, until creates outlast their timeout.
 */
export function removeSandbox(projectName: string): void {
  podman(LOCAL_CONNECTION, ["rm", "-f", `${projectName}-proxy`, projectName], { check: false });
  podman(LOCAL_CONNECTION, ["network", "rm", "-f", `mio-proxy-${projectName}`], { check: false });
}

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
    removeSandbox(projectName);
  });

  return { projectName, row, workspaceDir };
}
