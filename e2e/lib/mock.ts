/**
 * Typed helper for the scriptable mock backend's runtime control plane
 * (docs/features/scriptable-mock-backend/02_dev.md).
 */

/**
 * Mirrors `Behaviour` in src-tauri/src/mockctl/mod.rs (internally tagged on `kind`, kebab-case).
 * Field names are snake_case, matching the Rust struct fields verbatim — this payload crosses the
 * IPC boundary as plain serde JSON, not Tauri's camelCase command-argument convention.
 */
export type Behaviour =
  | { kind: "fail-once"; exit_code: number; stderr: string }
  | { kind: "fail-always"; exit_code: number; stderr: string }
  | { kind: "return-value"; stdout: string; exit_code?: number }
  | { kind: "return-sequence"; values: { stdout: string; exit_code?: number }[] };

/**
 * Invokes any Tauri command from the running webview. Rejects with the command's raw `AppError`
 * ({ kind, message }) on failure — callers that expect a rejection should catch and parse it
 * themselves (see `parseError` in src/lib/errors.ts).
 */
export async function invokeCmd<T = unknown>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return browser.execute(
    (c, a) => {
      // @ts-expect-error __TAURI_INTERNALS__ is injected into the webview at runtime.
      return window.__TAURI_INTERNALS__.invoke(c, a);
    },
    cmd,
    args,
  );
}

/**
 * Installs one scripted override. `key` follows the `<binary>.<subcommand>` convention
 * (`podman.ps`, `podman.start`, `podman.machine-list`) — see `KNOWN_CALLS` in
 * src-tauri/src/mockctl/mod.rs for the full list.
 */
export async function configureMock(key: string, behaviour: Behaviour): Promise<void> {
  await invokeCmd("mock_configure", { key, behaviour });
}

/**
 * Installs an override expected to be *rejected*, returning the raw AppError or null if
 * mock_configure unexpectedly succeeded. Catches inside the webview on purpose: a promise rejected
 * within browser.execute() reaches the spec as a stringified WebdriverIO error otherwise, losing `kind`.
 */
export async function configureMockExpectingError(
  key: string,
  behaviour: Behaviour,
): Promise<{ kind: string; message: string } | null> {
  return browser.execute(
    async (k, b) => {
      try {
        // @ts-expect-error __TAURI_INTERNALS__ is injected into the webview at runtime.
        await window.__TAURI_INTERNALS__.invoke("mock_configure", { key: k, behaviour: b });
        return null;
      } catch (e) {
        return e as { kind: string; message: string };
      }
    },
    key,
    behaviour,
  );
}

/** Clears every installed override. Safe to call even with none installed. */
export async function resetMocks(): Promise<void> {
  await invokeCmd("mock_reset");
}

/**
 * Registers `before`/`beforeEach`/`after` hooks at `describe` scope that reset the director.
 * Mandatory for any spec that calls configureMock(): the light suite runs one app session per spec
 * file, so an uncleared override otherwise leaks into the next `it` block or spec
 * (docs/features/scriptable-mock-backend/02_dev.md, Resolved Decision 3).
 *
 * The `beforeEach` reset means each `it` block must install every override it depends on itself —
 * installing overrides in a spec's own `before` hook will not survive to the tests.
 */
export function useMockControl(): void {
  before(resetMocks);
  beforeEach(resetMocks);
  after(resetMocks);
}
