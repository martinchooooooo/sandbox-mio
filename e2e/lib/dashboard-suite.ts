import workspaceDashboard from "../test/pageobjects/workspace-dashboard.page.js";
import { captureDiagOnFailure } from "./diag.js";

// Every spec whose tests operate on a sandbox's dashboard webview (package-autocomplete,
// expose-port, agent-setup, runtime-management, opencode-agent) copy-pasted the same pair of
// afterEach hooks: leave the session on the app webview between tests - the global `before`
// hook (e2e/wdio.conf.ts) only pins the session there once per session, and tests that don't touch
// the dashboard (navigateHome-based reloads, row.deployTo, row.destroy) assume that default - and
// dump page state to e2e/.diag on failure. The webview reset is dashboard-specific and stays here;
// the failure dump is shared with the standalone specs via captureDiagOnFailure() (lib/diag.ts).
// Call this once per `describe` block.
export function useDashboardWebview(): void {
  afterEach(async () => {
    await workspaceDashboard.switchToAppWebview().catch(() => {});
  });

  captureDiagOnFailure();
}

// Wraps a test body that needs sandbox `sandbox()`'s dashboard webview: switches to it before
// running `fn`. The afterEach above resets every test back to the app webview, so any test that
// touches dashboard elements has to re-attach - that manual step is exactly what
// package-autocomplete.spec.ts forgot for four tests in a row, and the failure it produced
// ("element wasn't found") pointed nowhere near the actual cause. Baking the switch into the
// wrapper means a test written with this helper cannot forget it.
//
// Do not use this for a suite's first dashboard-opening test: the pane does not exist until that
// test creates/opens the workspace, so it calls workspaceDashboard.switchToDashboard itself after
// doing so. Also skip it for tests that must start on the app webview (e.g. a navigateHome()
// reload) even though they switch to the dashboard partway through their own body.
export function dashboardIt(title: string, sandbox: () => string, fn: () => Promise<void>): void {
  it(title, async () => {
    await workspaceDashboard.switchToDashboard(sandbox());
    await fn();
  });
}
