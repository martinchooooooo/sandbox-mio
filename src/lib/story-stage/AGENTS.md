# Story Stage Orientation

`src/lib/story-stage/` is the whiteboard animation engine: the cast, placement, timing, screens, arms, effects and `stage.css`. Two consumers mount it: the onboarding book (`src/lib/onboarding/book/Stage.svelte`, a landscape 1000 x 440 board) and the native chat's live stage (`src/lib/agent-chat/LiveStage.svelte`, a portrait board, see `docs/features/chat-agent-anatomy/02_dev.md`). The root `AGENTS.md` and `src/AGENTS.md` rules apply here.

## Writing a scene

A scene is a function from the stage to `{ layout, setup?, play }` (`Scene` in `index.ts`). It runs afresh on every visit and replay, so state shared between `setup` and `play` lives in the function's closure.

- `layout` maps actor names (`you`, `chat`, `brain`, `pc`, `host`, `box`) to `[x, y, scale, opacity]` in stage coordinates. The book's board is `1000 x 440`; `createStage()` takes another size through `options.size`. An actor left out is hidden, and moving between pages animates.
- `setup` draws what the page starts with. `play` is the async script.
- The stage object is the whole drawing surface; its inferred type `StoryStage` lists every helper with a doc comment. Read `index.ts` first, then the file that owns the helper.
- Point at actors with `at(name, dx, dy)`, which converts an actor's own coordinates to stage coordinates. Never hard-code a position that depends on a layout.
- One short sentence per `cap()` call. The caption narrates; the page text explains. Do not repeat one in the other.
- The board keeps its light palette in dark mode. Reuse the `--s-*` tokens at the top of `stage.css`; the only other colours allowed are the brand fills in a chapter's `copy.ts`.
- A new thing that appears on screen gets its drawing helper here and its classes in `stage.css`, scoped under `.story-stage`.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| The previous page's animation keeps running after a page turn, or throws | A wait used raw `setTimeout`, `requestAnimationFrame` or a bare promise | Wait only through `sleep`, `tween`, `clicked` or helpers built on them; a page turn cancels those |
| Leftover bubbles, badges or arms when returning to a page | A drawing helper kept state outside the stage reset | Push a cleanup function onto `resets`, as `characters.ts` and `arms.ts` do |
| A group jumps to the stage's top-left corner while it fades in | An animation class with `transform` sits on an element positioned by a `transform` attribute | Put the class on an inner group, as `tool` in `tools.ts` does |
| `actor "…" is not in this page's layout` | `at()` or `reach()` used an actor the page's `layout` does not place | Add the actor to `layout`; arms always need `brain` |
| Hands hidden behind something the arms should hold | The thing was drawn in `layers.fx`, which paints above the arms | Draw held things in `layers.fxBack` |
| Arms vanish behind the host computer | The arms layer paints behind actors by default | Call `armsOver()` in the scene's `setup`; page turns restore the order |
| A scene's styles do not apply | The SVG is built imperatively, so Svelte's scoped styles cannot reach it | Put the rule in `stage.css` under `.story-stage` |
| An animation named `rise` or `fade` behaves differently from the standalone story | Keyframes are global, and the app has its own | Prefix every keyframe in `stage.css` with `story-` |
