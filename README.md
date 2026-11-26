# sandbox-mio

An app to help non-technical users learn how to use AI agents inside of a sandbox environment, hopefully safer than running programs on their machine.

## Prerequisites

- [Rust](https://rustup.rs)
- [pnpm](https://pnpm.io/installation)

## Development

The build tool is the `xtask` Cargo package, run as `cargo xtask <subcommand>` from the repository
root; the alias lives in `.cargo/config.toml` and the first run compiles the tool (a few seconds).
Run `cargo xtask` with no arguments for the usage text.

```sh
cargo xtask dev --setup
```

### Run the app

```sh
cargo xtask dev
```

Launches the app with hot reloading.

## E2E Tests

```sh
cargo xtask e2e --setup
```

Tests use [WebdriverIO](https://webdriver.io/) driving `msedgedriver` directly. For more info, read [here](https://v2.tauri.app/develop/tests/webdriver/example/webdriverio/)

### Running

```sh
cargo xtask e2e
```

This will run the `standard` tests by default. To select a different suite, pass it in as another arg

```sh
cargo xtask e2e light
```

The test suite builds the debug binary automatically before launching the app.

### CI

```sh
cargo xtask ci [checks|e2e-light|diagnostics]
```

Runs locally what `.github/workflows/ci.yml` runs; with no argument it runs `checks`, then `e2e-light`.

## Documentation checks

```sh
cargo xtask docs        # check
cargo xtask docs --fix  # regenerate every index.md from frontmatter, then check
```

Checks `docs/` against the documentation rules (OKF frontmatter, feature statuses, generated
indexes, the feature log, links, architecture-document size, and `docs/...` citations from code).
Every `index.md` except the root `docs/index.md` is generated; never edit one by hand. The rules
are code in `xtask/src/docs_check.rs`; its unit tests run with `cargo test` in `xtask/`.

## Packaging

```sh
cargo xtask package
```

Stages the bundled Mutagen/llama.cpp/Podman binaries (if not already present) and
builds a release NSIS installer under
`src-tauri/target/release/bundle/nsis/`, with the `devtools` feature dropped.

### Auto-updater setup (one-time, not done by this repo)

The app checks `plugins.updater.endpoints` (`src-tauri/tauri.conf.json`) on
launch and, via `.github/workflows/release.yml`, publishes releases to this
repo's GitHub Releases as `latest.json` + signed installer/updater artifacts.
Before the first tagged release:

1. Generate a signing keypair: `pnpm tauri signer generate -w ~/.tauri/sandbox-mio.key`
   (keep the private key + password out of the repo).
2. Replace `REPLACE_WITH_PUBKEY_FROM_TAURI_SIGNER_GENERATE` in
   `src-tauri/tauri.conf.json` (`plugins.updater.pubkey`) with the generated
   **public** key.
3. Add `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` as
   GitHub Actions repo secrets (Settings → Secrets and variables → Actions).
4. Push a `v*` tag to trigger `.github/workflows/release.yml`; it publishes a
   **draft** release — review and publish it manually.

## UI

Leverages shadcn components (written with Tailwind), that we try to leverage throughout the app as much as possible. Grab a component from here if in need of one, or if you believe a new one is needed, ass one with ` pnpm dlx shadcn-svelte@latest add ${component}`
