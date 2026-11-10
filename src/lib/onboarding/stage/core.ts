// Timing primitives every scene is written with. Each wait is cancellable by a page turn.

const NS = "http://www.w3.org/2000/svg";

export interface StageOptions {
    /** Click-along pages play themselves with a fake cursor. */
    auto?: boolean;
    /** Fast-forwards every sleep and tween. */
    speed?: number;
    /** Receives each caption line; an empty string clears it. */
    onCaption?: (text: string) => void;
}

/** Rejection value of a wait whose page was turned; the book swallows it. */
export const CANCEL = Symbol("cancel");

export function createCore(options: StageOptions) {
    const AUTO = options.auto ?? false;
    const SPEED = options.speed || 1;

    /** Create an SVG element, set attributes, optionally append it to `parent` and set its text. */
    function el<K extends keyof SVGElementTagNameMap>(
        tag: K,
        attrs: Record<string, string | number> = {},
        parent?: Element | null,
        text?: string,
    ): SVGElementTagNameMap[K] {
        const node = document.createElementNS(NS, tag) as SVGElementTagNameMap[K];
        for (const key in attrs) node.setAttribute(key, String(attrs[key]));
        if (text != null) node.textContent = text;
        if (parent) parent.appendChild(node);
        return node;
    }

    // Every wait captures `run`; turning the page bumps it, which rejects the old page's waits.
    let run = 0;
    const cancelAll = () => {
        run++;
    };

    const sleep = (ms: number) => {
        const id = run;
        return new Promise<void>((res, rej) =>
            setTimeout(() => (id === run ? res() : rej(CANCEL)), ms / SPEED),
        );
    };

    /** Calls fn(t) with eased t in 0..1 over `ms`. */
    const tween = (ms: number, fn: (t: number) => void) => {
        const id = run;
        return new Promise<void>((res, rej) => {
            const t0 = performance.now();
            const step = (now: number) => {
                if (id !== run) return rej(CANCEL);
                const t = Math.min(1, Math.max(0, ((now - t0) * SPEED) / ms));
                fn(t < 0.5 ? 2 * t * t : 1 - Math.pow(-2 * t + 2, 2) / 2);
                if (t < 1) requestAnimationFrame(step);
                else res();
            };
            step(t0);
        });
    };

    /** Resolves on the next click of `node`; this is how a page waits for the reader. */
    const clicked = (node: Element) => {
        const id = run;
        return new Promise<void>((res, rej) =>
            node.addEventListener("click", () => (id === run ? res() : rej(CANCEL)), { once: true }),
        );
    };

    return {
        AUTO,
        SPEED,
        CANCEL,
        el,
        sleep,
        tween,
        clicked,
        cancelAll,
        /** Callbacks run on every page turn, before the new page is drawn. */
        resets: [] as (() => void)[],
    };
}

export type Core = ReturnType<typeof createCore>;
