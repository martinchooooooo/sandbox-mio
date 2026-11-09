import { fly, scale, type TransitionConfig } from "svelte/transition";
import { quintOut } from "svelte/easing";

/**
 * Svelte transition presets for the app's motion system. Reach for these only when something must
 * animate *out* (CSS cannot delay an unmount); for enter-only motion use the `animate-enter`,
 * `animate-fade-in`, `animate-pop-in` and `stagger` utilities from `src/app.css`.
 *
 * Write `in:x out:x` rather than `transition:x`: the presets see the direction and make exits
 * faster than entrances, which `transition:` (direction "both") cannot.
 */

/** Milliseconds; must equal `--motion-fast/base/slow` in `src/app.css`. */
export const DURATION = { fast: 150, base: 200, slow: 280 } as const;

/** Never 0: WebDriver treats opacity 0 as hidden. Must equal the keyframes in `src/app.css`. */
const MIN_OPACITY = 0.01;

/** Equals `--ease-out-quint` in `src/app.css`. */
export const EASE = quintOut;

type Direction = { direction: "in" | "out" | "both" };

export interface MotionParams {
    delay?: number;
    /** Defaults to the matching CSS utility's duration for entrances, `fast` for exits. */
    duration?: number;
}

function reducedMotion(): boolean {
    return typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;
}

function durationFor(direction: Direction["direction"], entrance: number, duration?: number): number {
    return duration ?? (direction === "out" ? DURATION.fast : entrance);
}

/**
 * A surface arriving or leaving: fade plus a short slide (default 6px up, like `animate-enter`).
 * Pass `x` for panels that come from the side.
 */
export function rise(
    node: Element,
    { delay = 0, duration, x = 0, y = 6 }: MotionParams & { x?: number; y?: number } = {},
    { direction }: Direction = { direction: "both" },
): TransitionConfig {
    const still = reducedMotion();
    return fly(node, {
        delay,
        duration: durationFor(direction, DURATION.slow, duration),
        x: still ? 0 : x,
        y: still ? 0 : y,
        opacity: MIN_OPACITY,
        easing: EASE,
    });
}

/**
 * A floating element (popover, panel anchored to a trigger) growing from its anchor, like
 * `animate-pop-in`. Set the anchor with a Tailwind `origin-*` class on the node.
 */
export function pop(
    node: Element,
    { delay = 0, duration }: MotionParams = {},
    { direction }: Direction = { direction: "both" },
): TransitionConfig {
    return scale(node, {
        delay,
        duration: durationFor(direction, DURATION.base, duration),
        start: reducedMotion() ? 1 : 0.96,
        opacity: MIN_OPACITY,
        easing: EASE,
    });
}
