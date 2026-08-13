/** Which machine engine a welcome-flow readiness check targets (mirrors Rust `ProviderId`). */
export type ProviderId = "podman" | "microsandbox";

/** One row of a provider's readiness checklist (mirrors Rust `machine::types::SetupStep`). */
export interface SetupStep {
  id: string;
}

/** Streamed `prepare_machine` progress (mirrors Rust `machine::types::MachineSetupEvent`). */
export type MachineSetupEvent =
  | { kind: "stepStart"; id: string }
  | { kind: "stepDone"; id: string; detail: string | null }
  | { kind: "stepSkipped"; id: string; reason: string }
  | { kind: "stepFailed"; id: string; error: string }
  | { kind: "rebootRequired"; message: string }
  | { kind: "ready" };

/** Result of `machine_readiness` (mirrors Rust `machine::types::MachineReadiness`). */
export interface MachineReadiness {
  ready: boolean;
  needsSetup: boolean;
}
