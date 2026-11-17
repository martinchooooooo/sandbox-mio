import { readFileSync } from "fs";
import { fileURLToPath } from "url";

// Floating "latest" resolution for mise hits GitHub's 60 req/h unauthenticated rate limit and
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
// Pinned: an exact zellij semver the spike recipes' `mise install` interpolates.
const PINNED_ZELLIJ = /ARG\s+ZELLIJ_VERSION=[0-9]+\.[0-9]+\.[0-9]+\b/;
// Pinned: the shipped recipe downloads Herdr as a release asset at an exact semver, then verifies it.
const PINNED_HERDR = /ARG\s+HERDR_VERSION=[0-9]+\.[0-9]+\.[0-9]+/;
// Pinned: the checksum the download is verified against, a full SHA-256 digest.
const PINNED_HERDR_SHA256 = /ARG\s+HERDR_SHA256=[0-9a-f]{64}\s/;
const HERDR_CHECKSUM = /sha256sum -c[\s\S]*chmod \+x \/usr\/local\/bin\/herdr/;
// Floating forms that hit the rate-limited GitHub API: a `releases/latest` redirect, an `@latest`
// tool spec, or piping the `mise.run` installer. None may appear in a recipe.
const FLOATING = [/releases\/latest/, /@latest/, /mise\.run/];

// The recipe baked into the app; the other entries are spike docs.
const SHIPPED = CONTAINERFILES[0];

describe("Base image recipe", () => {
  for (const relPath of CONTAINERFILES) {
    const shipped = relPath === SHIPPED;
    it(`pins mise and ${shipped ? "herdr" : "zellij"} to exact versions in ${relPath}`, () => {
      const recipe = readFileSync(
        fileURLToPath(new URL(relPath, import.meta.url)),
        "utf8"
      );

      expect(recipe).toMatch(PINNED_MISE);
      if (shipped) {
        expect(recipe).toMatch(PINNED_HERDR);
        expect(recipe).toMatch(PINNED_HERDR_SHA256);
        expect(recipe).toMatch(HERDR_CHECKSUM);
      } else {
        expect(recipe).toMatch(PINNED_ZELLIJ);
      }
      for (const floating of FLOATING) {
        expect(recipe).not.toMatch(floating);
      }
    });
  }
});
