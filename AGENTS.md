
# UI Components

This project uses [shadcn-svelte](https://shadcn-svelte.com) for UI primitives. Components live in `src/lib/components/ui/`.

When building UI features, always check `src/lib/components/ui/` first and reach for an existing component before writing custom markup.

Currently available components:
- `button`
- `field` (Field.Set, Field.Group, Field.Field, Field.Label, Field.Description, Field.Error, etc.)
- `input`
- `label`
- `separator`

## Adding new components

If a shadcn-svelte component would be a good fit but isn't in `src/lib/components/ui/` yet, ask the user if they'd like to add it before building a custom alternative. Components can be added via:

```
pnpm dlx shadcn-svelte@latest add <component-name>
```
