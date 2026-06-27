import { readFileSync } from "fs";
import { fileURLToPath } from "url";

// Regression guard for the base-image build failing at the flox step. `nix profile add
// github:flox/flox` resolves the floating HEAD through the GitHub commits API, which is rate-limited
// to 60 requests/h per unauthenticated IP; once the machine's egress IP trips that limit the build
// dies with "HTTP error 403 ... API rate limit exceeded" — non-deterministically, and the floating
// ref is non-reproducible regardless. The fix pins flox to a full 40-hex commit so nix fetches
// archive/<rev>.tar.gz directly, with no API call. This static guard asserts the recipe stays
// pinned; it needs no podman machine, so it is cheap to keep alongside the full build spec.
//
// Two copies of the recipe must stay in lockstep: the one baked into the binary (CONTAINERFILE in
// src-tauri/src/backends/mod.rs) and the spike reference that e2e provisioning builds from
// (ensureFloxBaseImage in e2e/lib/podman-machine.ts). Both are checked.
const CONTAINERFILES = [
  "../../../src-tauri/resources/Containerfile",
  "../../../docs/features/sandbox-runtime-management/spike/Containerfile",
];

// Both patterns anchor on the `nix profile add` directive so a prose mention of `github:flox/flox`
// in a recipe comment cannot trip them.
// Pinned: `nix profile add github:flox/flox/<rev>` where <rev> is exactly 40 hex chars — the form
// that skips the GitHub API.
const PINNED_FLOX = /nix profile add\s+github:flox\/flox\/[0-9a-f]{40}\b/;
// Floating: the HEAD form the bug shipped with — `nix profile add github:flox/flox` with no `/rev`.
const FLOATING_FLOX = /nix profile add\s+github:flox\/flox(?![/0-9a-zA-Z])/;

// The zellij web-terminal engine is baked the same way and carries the same rate-limit risk: the
// floating `nixpkgs#zellij` flakeref resolves through the GitHub API (60 req/h unauthenticated) and
// fails the build with HTTP 403, while a full nixpkgs rev fetches archive/<rev>.tar.gz directly.
// Only the binary-baked recipe (src-tauri/resources/Containerfile) bakes zellij, so this guard runs
// against that file alone — the sandbox-runtime-management spike recipe predates the terminal.
const ZELLIJ_RECIPE = "../../../src-tauri/resources/Containerfile";
// Pinned: `nix profile add github:NixOS/nixpkgs/<40-hex>#zellij`.
const PINNED_ZELLIJ = /nix profile add\s+github:NixOS\/nixpkgs\/[0-9a-f]{40}#zellij\b/;
// Floating: the bare `nixpkgs#zellij` flakeref that hits the rate-limited API.
const FLOATING_ZELLIJ = /nix profile add\s+nixpkgs#zellij\b/;

describe("Base image recipe", () => {
  for (const relPath of CONTAINERFILES) {
    it(`pins flox to an immutable revision in ${relPath}`, () => {
      const recipe = readFileSync(
        fileURLToPath(new URL(relPath, import.meta.url)),
        "utf8"
      );

      expect(recipe).toMatch(PINNED_FLOX);
      expect(recipe).not.toMatch(FLOATING_FLOX);
    });
  }

  it(`pins zellij to an immutable nixpkgs revision in ${ZELLIJ_RECIPE}`, () => {
    const recipe = readFileSync(
      fileURLToPath(new URL(ZELLIJ_RECIPE, import.meta.url)),
      "utf8"
    );

    expect(recipe).toMatch(PINNED_ZELLIJ);
    expect(recipe).not.toMatch(FLOATING_ZELLIJ);
  });
});
