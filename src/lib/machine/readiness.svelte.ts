import { parseError, type AppErrorKind } from "$lib/errors";
import { fetchMachineReadiness, fetchMachineSetupSteps } from "./readiness";
import type { MachineReadiness, ProviderId, SetupStep } from "./types";

/**
 * One machine-readiness result per provider, shared by every card that needs it.
 *
 * The welcome flow renders two cards that mount together, and a readiness probe costs a real
 * `podman machine list`; owning one in-flight request here is what keeps them from firing their own
 * (`docs/features/podman-process-isolation/02_dev.md`, Resolved Decision 3). Module-scoped rather
 * than component-local because component `$state` is destroyed on navigation (`src/AGENTS.md`).
 */
interface ReadinessState {
    probing: boolean;
    readiness: MachineReadiness;
    steps: SetupStep[];
    error: string;
    /** Kind of the last probe failure, so a caller can render "podman never answered" distinctly. */
    errorKind: AppErrorKind | null;
}

function empty(): ReadinessState {
    return {
        probing: true,
        readiness: { ready: false, needsSetup: false },
        steps: [],
        error: "",
        errorKind: null,
    };
}

let byProvider = $state<Record<string, ReadinessState>>({});
const inFlight = new Map<ProviderId, Promise<void>>();

/** Current state for `provider`; the untouched initial state (still probing) until one is loaded. */
export function machineReadiness(provider: ProviderId): ReadinessState {
    return byProvider[provider] ?? empty();
}

/**
 * Probe `provider` unless the same probe is already running, in which case the caller joins it.
 * `force` re-probes even when a result is already held — what a retry control and a completed setup
 * run both need.
 */
export function ensureMachineReadiness(provider: ProviderId, force = false): Promise<void> {
    const running = inFlight.get(provider);
    if (running) return running;
    if (!force && byProvider[provider] && !byProvider[provider].probing) return Promise.resolve();

    const probe = (async () => {
        byProvider = { ...byProvider, [provider]: { ...machineReadiness(provider), probing: true } };
        try {
            const [steps, readiness] = await Promise.all([
                fetchMachineSetupSteps(provider),
                fetchMachineReadiness(provider),
            ]);
            byProvider = {
                ...byProvider,
                [provider]: { probing: false, readiness, steps, error: "", errorKind: null },
            };
        } catch (e) {
            const err = parseError(e);
            byProvider = {
                ...byProvider,
                [provider]: {
                    ...machineReadiness(provider),
                    probing: false,
                    error: err.message,
                    errorKind: err.kind,
                },
            };
        } finally {
            inFlight.delete(provider);
        }
    })();
    inFlight.set(provider, probe);
    return probe;
}
