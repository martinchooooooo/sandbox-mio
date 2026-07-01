import { readFileSync } from "fs";
import { fileURLToPath } from "url";

// Regression guard for the base-image build failing on a floating version. mise and zellij are
// fetched from GitHub releases; resolving "latest" (the `releases/latest` redirect, a `mise@latest`
// install, or the `mise.run` script) hits the GitHub API, which is rate-limited to 60 requests/h per
// unauthenticated IP — once the machine's egress IP trips that limit the build dies with HTTP 403,
// non-deterministically, and a floating version is non-reproducible regardless. The fix pins both
// the mise release (ARG MISE_VERSION=v…) and the zellij version (ARG ZELLIJ_VERSION=…) to exact
// tags so the downloads address a known artifact with no version-listing call. This static guard
// asserts the recipes stay pinned; it needs no podman machine, so it is cheap to keep.
//
// The recipe baked into the binary (CONTAINERFILE in src-tauri/src/backends/mod.rs) and the spike
// references that document the build must stay in lockstep. All are checked.
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
