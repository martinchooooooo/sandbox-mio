import { MODELS } from "./catalogue";
import type { MachineSpecs, ModelDescriptor, ModelRating } from "./types";

export interface RankedModel {
  descriptor: ModelDescriptor;
  rating: ModelRating;
}

const GB = 1024 * 1024 * 1024;

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
