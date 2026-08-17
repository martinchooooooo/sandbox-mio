# Deferred Improvements

## `GlobalNav` toolbar/sidebar restyle

`src/lib/components/GlobalNav.svelte` renders its five routes (`intro`,
`projects`, `models`, `settings`, `credentials`) as `Tabs.List`/`Tabs.Trigger`
(`src/lib/components/ui/tabs/`), styled as browser-style tabs. That reads more
"web app" than "desktop app" alongside the rest of the changes in
`01_ideas.md`, but it is a visual restyle of a component every routed page
depends on, independent of the disclosure/relocation work in `01_ideas.md`.
Deferred to its own scoped feature rather than riding along with the
Advanced-disclosure and sandbox-list changes.
