import assert from "node:assert";

import {
  forwardList,
  forwardTerminate,
  SWEEP_ORPHAN_SESSION,
  SWEEP_LEGACY_TERMINAL_SESSION,
  SWEEP_KEEP_SESSION,
} from "../../../lib/mutagen.js";

function sessionGone(out: string): boolean {
  return out === "" || out.includes("did not match any sessions");
}

describe("Startup sweep of stale forwards", () => {
  after(() => {
    forwardTerminate(SWEEP_KEEP_SESSION);
    forwardTerminate(SWEEP_ORPHAN_SESSION);
    forwardTerminate(SWEEP_LEGACY_TERMINAL_SESSION);
  });

  it("terminates the orphaned agentweb forward within 60s of startup", async () => {
    await browser.waitUntil(() => sessionGone(forwardList(SWEEP_ORPHAN_SESSION)), {
      timeout: 60000,
      interval: 2000,
      timeoutMsg:
        `orphan session ${SWEEP_ORPHAN_SESSION} still exists 60s after startup. Output:\n` +
        `${forwardList(SWEEP_ORPHAN_SESSION)}\n` +
        "sweep_stale_forwards must terminate agentweb- sessions with no live sandbox.",
    });
  });

  it("terminates every legacy terminal- forward within 60s of startup", async () => {
    await browser.waitUntil(() => sessionGone(forwardList(SWEEP_LEGACY_TERMINAL_SESSION)), {
      timeout: 60000,
      interval: 2000,
      timeoutMsg:
        `legacy session ${SWEEP_LEGACY_TERMINAL_SESSION} still exists 60s after startup. Output:\n` +
        `${forwardList(SWEEP_LEGACY_TERMINAL_SESSION)}\n` +
        "sweep_stale_forwards must terminate every terminal- session, live sandbox or not " +
        "(docs/features/native-terminal/02_dev.md, Resolved Decision 13).",
    });
  });

  it("never touches sessions outside the swept prefixes", () => {
    const out = forwardList(SWEEP_KEEP_SESSION);
    assert(
      !sessionGone(out) && out.includes("Identifier:"),
      `session ${SWEEP_KEEP_SESSION} disappeared — the sweep must only match the terminal-, ` +
        `agentweb-, expose- and inference- prefixes. Output:\n${out}`
    );
  });
});
