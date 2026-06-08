# sandbox-mio

An app to help non-technical users learn how to use AI agents inside of a sandbox environment, hopefully safer than running programs on their machine.

## Prerequisites

- [Rust](https://rustup.rs)
- [pnpm](https://pnpm.io/installation)

## Development

### install

`pnpm install`

During development `pnpm run tauri dev`, launches the app and then supports hot reloading.    

## E2E Tests

Tests use [WebdriverIO](https://webdriver.io/) via `tauri-driver`. For more info, read [here](https://v2.tauri.app/develop/tests/webdriver/example/webdriverio/)

### First-time setup

```sh
rustc build.rs && ./build
```

On Windows:

```powershell
rustc build.rs; .\build.exe
```

### Running

```sh
cd e2e
pnpm test
```

The test suite builds the debug binary automatically before launching the app.

## UI

Leverages shadcn components (written with Tailwind), that we try to leverage throughout the app as much as possible. Grab a component from here if in need of one, or if you believe a new one is needed, ass one with ` pnpm dlx shadcn-svelte@latest add ${component}`
