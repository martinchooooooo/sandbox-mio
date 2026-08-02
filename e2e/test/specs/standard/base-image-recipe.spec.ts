import { readFileSync } from "fs";
import { fileURLToPath } from "url";

// Floating "latest" resolution for mise/zellij hits GitHub's 60 req/h unauthenticated rate limit and
// fails the build non-deterministically; pinning exact versions avoids the version-listing call.
// This static guard needs no podman machine, so it is cheap to run on every recipe.
//
// The recipe baked into the binary (CONTAINERFILE in src-tauri/src/backends/mod.rs) and the spike
// docs that describe the build must stay pinned in lockstep — all are checked here.
const CONTAINERFILES = [
  "../../../../src-tauri/resources/Containerfile",
  "../../../../docs/features/sandbox-runtime-management/spike/mise/Containerfile.glibc",
  "../../../../docs/features/sandbox-runtime-management/spike/mise/Containerfile.musl",
];

// Pinned: an exact mise release tag (`ARG MISE_VERSION=v2026.6.14`) the curl download interpolates.
const PINNED_MISE = /ARG\s+MISE_VERSION=v[0-9][0-9.]*\b/;
// Pinned: an exact zellij semver (`ARG ZELLIJ_VERSION=0.44.3`) the `mise install` interpolates.
const PINNED_ZELLIJ = /ARG\s+ZELLIJ_VERSION=[0-9]+\.[0-9]+\.[0-9]+\b/;
// Floating forms that hit the rate-limited GitHub API: a `releases/latest` redirect, an `@latest`
// tool spec, or piping the `mise.run` installer. None may appear in a recipe.
const FLOATING = [/releases\/latest/, /@latest/, /mise\.run/];

describe("Base image recipe", () => {
  for (const relPath of CONTAINERFILES) {
    it(`pins mise and zellij to exact versions in ${relPath}`, () => {
      const recipe = readFileSync(
        fileURLToPath(new URL(relPath, import.meta.url)),
        "utf8"
      );

      expect(recipe).toMatch(PINNED_MISE);
      expect(recipe).toMatch(PINNED_ZELLIJ);
      for (const floating of FLOATING) {
        expect(recipe).not.toMatch(floating);
      }
    });
  }
});
