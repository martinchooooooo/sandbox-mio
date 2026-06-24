/** Discriminant tags matching `AppError` variants in `src-tauri/src/error.rs`. */
export type AppErrorKind = "Backend" | "Sync" | "Runtime" | "NotDeployed" | "UnsyncedChanges";

/** Structured error sent over the Tauri IPC boundary from every `#[tauri::command]`. */
export interface AppError {
    kind: AppErrorKind;
    message: string;
}

/**
 * Parse the value thrown by a failed `invoke()` call into an `AppError`.
 *
 * Tauri serialises `Err(AppError)` as `{ kind, message }`. Any other shape (e.g. a network
 * error before the IPC reaches Rust) falls back to `kind: "Backend"` with a string message so
 * callers can always treat the result uniformly.
 */
export function parseError(e: unknown): AppError {
    if (
        e !== null &&
        typeof e === "object" &&
        "kind" in e &&
        "message" in e &&
        typeof (e as Record<string, unknown>).kind === "string" &&
        typeof (e as Record<string, unknown>).message === "string"
    ) {
        return e as AppError;
    }
    return { kind: "Backend", message: String(e) };
}
