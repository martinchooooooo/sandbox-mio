# Onboarding Orientation

`src/lib/onboarding/` holds the illustrated onboarding book the welcome flow shows on first run (`src/lib/welcome/WelcomeFlow.svelte`), and the store that runs first-run setup in the background while the user reads. The design and its rationale are in `docs/features/onboarding-story/02_dev.md`. The root `AGENTS.md` and `src/AGENTS.md` rules apply here.

## Layout

| Path | Owns |
|---|---|
| `chapters.ts` | The ordered list of chapters the book shows. The only file a new chapter must touch outside its own directory. |
| `chapters/<chapter>/` | One chapter: a Svelte component made of `<Page>`s, plus its scene scripts and shared copy. |
| `book/` | The reusable kit every chapter is written with: `Book`, `Chapter`, `Act`, `Page`, `Term`, `Glossary`, `GlossaryItem`, `Callout`. Import from `book/index.ts`. |
| `stage/` | The whiteboard animation engine scenes draw with: the cast, placement, timing, screens, arms, effects, and `stage.css`. |
| `OnboardingBook.svelte` | Renders every registered chapter in one `Book`. |
| `setup.svelte.ts` | Background first-run setup (machine, then base image), shared with the two setup cards in `src/lib/welcome/`. |

## Adding a chapter

A fork adds its own onboarding material as a new chapter, without editing the existing one.

1. Create `chapters/<your-chapter>/<YourChapter>.svelte`. Write it with the kit only: wrap pages in `<Act label>` to group them, give each `<Page title>` its explanation as children, and mark key terms with `<Term>`. Use `<Callout>` for an aside and `<Glossary>` with `<GlossaryItem term>` for definitions.
2. Append the chapter to the list in `chapters.ts` with an `id`, a `title` function and the component. Page order is source order inside the component; chapter order is list order.
3. A page without a `scene` is text-only: the stage collapses and the text takes the whole card. Start there; add animation only where it explains something the text cannot.
4. Verify with `pnpm check`, then open the app with onboarding reset (the "Restart onboarding" button on `/`) or the `/intro` tab.

`chapters/agents-and-sandboxes/` is the worked example: `AgentsAndSandboxes.svelte` holds the text, the `act*.scenes.ts` files hold the animation, and `copy.ts` holds strings and product names shared by both.

## Writing a scene

A scene is a function from the stage to `{ layout, setup?, play }` (`Scene` in `stage/index.ts`), passed to `<Page scene>`. It runs afresh on every visit and replay, so state shared between `setup` and `play` lives in the function's closure.

- `layout` maps actor names (`you`, `chat`, `brain`, `pc`, `host`, `box`) to `[x, y, scale, opacity]` in stage coordinates. The stage is `1000 x 440`. An actor left out is hidden, and moving between pages animates.
- `setup` draws what the page starts with. `play` is the async script.
- The stage object is the whole drawing surface; its inferred type `StoryStage` lists every helper with a doc comment. Read `stage/index.ts` first, then the file that owns the helper.
- Point at actors with `at(name, dx, dy)`, which converts an actor's own coordinates to stage coordinates. Never hard-code a position that depends on a layout.
- One short sentence per `cap()` call. The caption narrates; the page text explains. Do not repeat one in the other.
- The board keeps its light palette in dark mode. Reuse the `--s-*` tokens at the top of `stage/stage.css`; the only other colours allowed are the brand fills in a chapter's `copy.ts`.
- A new thing that appears on screen gets its drawing helper in `stage/` and its classes in `stage/stage.css`, scoped under `.story-stage`.

## Translations

Onboarding strings have their own Paraglide files, `messages/onboarding/{locale}.json`, merged with `messages/{locale}.json` by the path patterns in `project.inlang/settings.json`. Keys start with `onboarding_` and are called through the same `m` object as every other key.

Only `onboarding_story_title` exists so far. Every other string here is a hard-coded English literal under a `TODO(i18n)` marker while the copy is being refined; the translation pass is Phase F of `docs/features/onboarding-story/02_dev.md`. Until that pass runs, write new chapter text as English literals too.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| The previous page's animation keeps running after a page turn, or throws | A wait used raw `setTimeout`, `requestAnimationFrame` or a bare promise | Wait only through `sleep`, `tween`, `clicked` or helpers built on them; a page turn cancels those |
| Leftover bubbles, badges or arms when returning to a page | A drawing helper kept state outside the stage reset | Push a cleanup function onto `resets`, as `stage/characters.ts` and `stage/arms.ts` do |
| `actor "…" is not in this page's layout` | `at()` or `reach()` used an actor the page's `layout` does not place | Add the actor to `layout`; arms always need `brain` |
| A group jumps to the stage's top-left corner while it fades in | An animation class with `transform` sits on an element positioned by a `transform` attribute | Put the class on an inner group, as `tool` in `stage/tools.ts` does |
| Hands hidden behind something the arms should hold | The thing was drawn in `layers.fx`, which paints above the arms | Draw held things in `layers.fxBack` |
| Arms vanish behind the host computer | The arms layer paints behind actors by default | Call `armsOver()` in the scene's `setup`; page turns restore the order |
| Pages appear in the wrong order | A page registered from `onMount`, which runs children first | Keep registration in `Page.svelte`'s init script, and write pages in reading order |
| A scene's styles do not apply | The SVG is built imperatively, so Svelte's scoped styles cannot reach it | Put the rule in `stage/stage.css` under `.story-stage` |
| An animation named `rise` or `fade` behaves differently from the standalone story | Keyframes are global, and the app has its own | Prefix every keyframe in `stage/stage.css` with `story-` |
| The setup cards and the status bar disagree, or setup runs twice | A component kept its own busy or error state instead of reading `setup.svelte.ts` | Read `machineRun()` and `imageRun()`; start work only through the store's functions |
