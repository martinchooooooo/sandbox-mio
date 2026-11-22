<script lang="ts">
    import type { Snippet } from "svelte";
    import type { Role } from "./anatomy";

    let {
        role,
        selected = false,
        detail = "",
        onselect,
        children,
    }: {
        role: Role;
        selected?: boolean;
        /** The tool word an Arms badge names. */
        detail?: string;
        onselect: () => void;
        children: Snippet;
    } = $props();

    const BADGES: Record<Role, string> = {
        "brain-think": "Brain · thinking", // TODO(i18n)
        "brain-reply": "Brain · reply", // TODO(i18n)
        "brain-ask": "Brain · asks", // TODO(i18n)
        "arms-act": "Arms", // TODO(i18n)
    };
    const label = $derived(role === "arms-act" && detail ? `${BADGES[role]} · ${detail}` : BADGES[role]);
    const isBrain = $derived(role !== "arms-act");
</script>

<div
    class="relative mt-2 rounded-md border-2 border-dashed px-3 pt-4 pb-2 {isBrain ? 'border-brain' : 'border-arms'} {selected
        ? 'border-solid ring-2 ' + (isBrain ? 'ring-brain/40' : 'ring-arms/40')
        : ''}"
    data-role-frame={role}
    data-selected={selected}
>
    <button
        type="button"
        class="absolute -top-3 left-2 cursor-pointer rounded-md border-2 px-1.5 text-[11px] leading-5 font-bold {isBrain
            ? 'border-brain bg-brain text-brain-foreground'
            : 'border-arms bg-arms text-arms-foreground'}"
        aria-pressed={selected}
        onclick={onselect}
        data-role-badge
    >
        {label}
    </button>
    {@render children()}
</div>
