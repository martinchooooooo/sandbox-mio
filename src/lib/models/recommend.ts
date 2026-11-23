import { GB, MODELS } from "./catalogue";
import type { EngineKind, FitGroup, FitVerdict, MachineSpecs, ModelDescriptor, ModelRating } from "./types";

export interface RankedModel {
  descriptor: ModelDescriptor;
  rating: ModelRating;
}

/**
 * Pure ranking of the catalogue against probed host specs. Reasons over free RAM headroom:
 * "recommended" when free RAM ≥ floor, "slow" within 2 GB below the floor, "wont-fit" otherwise.
 * Recommended-first, then by descending parameter count. Issues no IPC.
 */
export function recommendModels(specs: MachineSpecs): RankedModel[] {
  const free = specs.freeRamBytes;
  const ranked = MODELS.map((descriptor) => ({
    descriptor,
    rating: ratingFor(free, descriptor.ramFloorBytes),
  }));
  const order: Record<ModelRating, number> = { recommended: 0, slow: 1, "wont-fit": 2 };
  return ranked.sort(
    (a, b) =>
      order[a.rating] - order[b.rating] || b.descriptor.paramsB - a.descriptor.paramsB,
  );
}

export function ratingFor(freeRamBytes: number, ramFloorBytes: number): ModelRating {
  if (freeRamBytes >= ramFloorBytes) return "recommended";
  if (freeRamBytes >= ramFloorBytes - 2 * GB) return "slow";
  return "wont-fit";
}

export const roundGb = (bytes: number) => Math.round(bytes / GB);

export interface Fit {
  verdict: FitVerdict;
  group: FitGroup;
  freeGb: number;
  totalGb: number;
  needGb: number;
}

const GROUPS: Record<FitVerdict, FitGroup> = {
  fits: "runs-well",
  tight: "runs-slowly",
  busy: "needs-more",
  "too-small": "needs-more",
  engine: "needs-more",
};

/** Whether `engine` can serve the model at all. */
export function engineServes(d: ModelDescriptor, engine: EngineKind): boolean {
  return engine === "foundry" ? d.foundryAlias !== null : d.llama !== null;
}

/** Will the model run here, and if not, why. Derives from `ratingFor()` so the two never disagree. */
export function fitFor(specs: MachineSpecs, d: ModelDescriptor, engine: EngineKind): Fit {
  const rating = ratingFor(specs.freeRamBytes, d.ramFloorBytes);
  let verdict: FitVerdict;
  if (engine === "foundry" && d.foundryAlias === null) verdict = "engine";
  else if (rating === "recommended") verdict = "fits";
  else if (rating === "slow") verdict = "tight";
  // Firmware and GPU reservations leave a nominal 32 GB machine just under 32 GiB total.
  else verdict = specs.totalRamBytes >= d.ramFloorBytes - GB ? "busy" : "too-small";
  return {
    verdict,
    group: GROUPS[verdict],
    freeGb: roundGb(specs.freeRamBytes),
    totalGb: roundGb(specs.totalRamBytes),
    needGb: roundGb(d.ramFloorBytes),
  };
}

/** The model the page would wake: the test model, else the best one rated "recommended" the engine can serve. */
export function pickFor(
  specs: MachineSpecs,
  engine: EngineKind,
  testModel: ModelDescriptor | null = null,
): ModelDescriptor | null {
  if (testModel) return testModel;
  const top = recommendModels(specs).find((r) => r.rating === "recommended" && engineServes(r.descriptor, engine));
  return top?.descriptor ?? null;
}
