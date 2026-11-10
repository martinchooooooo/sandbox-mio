import { createArms } from "./arms";
import { createBoard, type Layout } from "./board";
import { createCharacters } from "./characters";
import { createComputers } from "./computers";
import { createCore, type StageOptions } from "./core";
import { createEffects } from "./effects";
import { createScreens } from "./screens";
import { createTools } from "./tools";

export { CANCEL, type StageOptions } from "./core";
export type { Layout, Placement } from "./board";
export type { Look } from "./characters";
export type { Mark } from "./arms";
export type { Computer } from "./computers";
export type { FileIcon, ScreenButton } from "./screens";
export type { ToolKind } from "./tools";

/**
 * Builds the whiteboard inside `svg` and returns everything a scene draws with: the cast (`you`,
 * `chat`, `brain`, `pc`, `host`, `box`), placement, timing, screens, arms and effects.
 * One instance per mounted `Stage.svelte`; nothing is shared between instances.
 */
export function createStage(svg: SVGSVGElement, options: StageOptions = {}) {
    const core = createCore(options);
    const board = createBoard(svg, core, options.onCaption ?? (() => {}));
    const characters = createCharacters(core, board);
    const computers = createComputers(core, board);
    const tools = createTools(core);
    const screens = createScreens(core, board);
    const arms = createArms(core, board, computers);
    const effects = createEffects(core, board);
    return { ...core, ...board, ...characters, ...computers, ...tools, ...screens, ...arms, ...effects };
}

/** The drawing and timing surface handed to every scene. */
export type StoryStage = ReturnType<typeof createStage>;

/** What a scene factory returns: where the cast stands, what is on screen at the start, and the script. */
export interface SceneScript {
    /** Actor name to placement; actors left out are hidden. Moving between pages animates. */
    layout: Layout;
    /** Runs synchronously after layout, to draw what the page starts with. */
    setup?: () => void;
    /** The animation. Every wait must go through `sleep`, `tween`, `clicked` or helpers built on them. */
    play: () => Promise<void>;
}

/**
 * One page's animation. Called afresh on every visit and replay, so locals shared between
 * `setup` and `play` live in the factory's closure.
 */
export type Scene = (s: StoryStage) => SceneScript;
