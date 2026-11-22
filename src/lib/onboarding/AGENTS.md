# Onboarding Orientation

`src/lib/onboarding/` holds the illustrated onboarding book the welcome flow shows on first run (`src/lib/welcome/WelcomeFlow.svelte`), and the store that runs first-run setup in the background while the user reads. The design and its rationale are in `docs/features/onboarding-story/02_dev.md`. The root `AGENTS.md` and `src/AGENTS.md` rules apply here.

## Layout

| Path | Owns |
|---|---|
| `chapters.ts` | The ordered list of chapters the book shows. The only file a new chapter must touch outside its own directory. |
| `chapters/<chapter>/` | One chapter: a Svelte component made of `<Page>`s, plus its scene scripts and shared copy. |
| `book/` | The reusable kit every chapter is written with: `Book`, `Chapter`, `Act`, `Page`, `Term`, `Glossary`, `GlossaryItem`, `Callout`. Import from `book/index.ts`. |
| `../story-stage/` | The whiteboard animation engine scenes draw with, shared with the native chat's live stage; see `src/lib/story-stage/AGENTS.md`. |
| `OnboardingBook.svelte` | Renders every registered chapter in one `Book`. |
| `setup.svelte.ts` | Background first-run setup (machine, base image, model and the first sandbox), shared with the setup progress page in `src/lib/welcome/`. `handoff.svelte.ts` opens the first sandbox once setup is ready. |

## Adding a chapter

A fork adds its own onboarding material as a new chapter, without editing the existing one.

1. Create `chapters/<your-chapter>/<YourChapter>.svelte`. Write it with the kit only: wrap pages in `<Act label>` to group them, give each `<Page title>` its explanation as children, and mark key terms with `<Term>`. Use `<Callout>` for an aside and `<Glossary>` with `<GlossaryItem term>` for definitions.
2. Append the chapter to the list in `chapters.ts` with an `id`, a `title` function and the component. Page order is source order inside the component; chapter order is list order.
3. A page without a `scene` is text-only: the stage collapses and the text takes the whole card. Start there; add animation only where it explains something the text cannot.
4. Verify with `pnpm check`, then open the app with onboarding reset (Settings → Debugging → "Restart onboarding") or the `/intro` tab.

`chapters/agents-and-sandboxes/` is the worked example: `AgentsAndSandboxes.svelte` holds the text, the `act*.scenes.ts` files hold the animation, and `copy.ts` holds strings and product names shared by both.

## Writing a scene

Scenes are written against the engine in `src/lib/story-stage/`; its `AGENTS.md` holds the scene-writing rules. A scene is passed to `<Page scene>`.

## Translations

Onboarding strings have their own Paraglide files, `messages/onboarding/{locale}.json`, merged with `messages/{locale}.json` by the path patterns in `project.inlang/settings.json`. Keys start with `onboarding_` and are called through the same `m` object as every other key.

Only `onboarding_story_title` exists so far. Every other string here is a hard-coded English literal under a `TODO(i18n)` marker while the copy is being refined; the translation pass is Phase F of `docs/features/onboarding-story/02_dev.md`. Until that pass runs, write new chapter text as English literals too.

## Common Gotchas

| Gotcha | Cause | Fix |
|---|---|---|
| Pages appear in the wrong order | A page registered from `onMount`, which runs children first | Keep registration in `Page.svelte`'s init script, and write pages in reading order |
| The setup cards and the status bar disagree, or setup runs twice | A component kept its own busy or error state instead of reading `setup.svelte.ts` | Read `machineRun()`, `imageRun()`, `modelRun()` and `sandboxRun()`; start work only through the store's functions |
