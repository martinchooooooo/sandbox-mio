import { GB } from "./catalogue";
import { SPEED_STEPS, sizeLabel, speedLabel } from "./copy";
import type { Fit } from "./recommend";
import type { MachineSpecs, ModelDescriptor } from "./types";

// Size buckets end below the limit; speed buckets end at it.
const SIZE_LIMITS_B = [5, 15, 25];
const SPEED_LIMITS_B = [4, 12];

export interface ModelScales {
    size: string;
    /** Null where a speed would mislead: the Model does not fit this computer. */
    speed: string | null;
    downloadGb: number;
    memoryGb: number;
}

/** Plain-language scales for a card; the speed is a heuristic, never a measurement. */
export function describeModel(d: ModelDescriptor, specs: MachineSpecs, fit: Fit): ModelScales {
    let speedStep = SPEED_LIMITS_B.filter((l) => d.activeParamsB > l).length;
    if (specs.gpuNames.length > 0) speedStep -= 1;
    if (fit.verdict === "tight") speedStep += 1;
    speedStep = Math.min(SPEED_STEPS - 1, Math.max(0, speedStep));
    return {
        size: sizeLabel(SIZE_LIMITS_B.filter((l) => d.paramsB >= l).length),
        speed: fit.group === "needs-more" ? null : speedLabel(speedStep),
        downloadGb: Math.round((d.llama?.sizeBytes ?? 0) / GB),
        memoryGb: fit.needGb,
    };
}
