import { Channel, invoke } from "@tauri-apps/api/core";

/** Mirrors Rust `TerminalEvent` in `src-tauri/src/terminal/types.rs`. */
export type TerminalEvent = { type: "exited"; reason: string; exitStatus: number | null };

/** Output bytes arrive on `onOutput` as an `ArrayBuffer` (or a number array on older webviews). */
export function terminalAttach(
    name: string,
    cols: number,
    rows: number,
    onOutput: Channel<ArrayBuffer | number[]>,
    onEvent: Channel<TerminalEvent>,
): Promise<void> {
    return invoke("terminal_attach", { name, cols, rows, onOutput, onEvent });
}

export function terminalInput(name: string, data: string): Promise<void> {
    return invoke("terminal_input", { name, data });
}

export function terminalResize(name: string, cols: number, rows: number): Promise<void> {
    return invoke("terminal_resize", { name, cols, rows });
}
