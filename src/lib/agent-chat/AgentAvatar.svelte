<script lang="ts">
    import BrainIcon from "@lucide/svelte/icons/brain";
    import FilePenIcon from "@lucide/svelte/icons/file-pen";
    import FileTextIcon from "@lucide/svelte/icons/file-text";
    import HandIcon from "@lucide/svelte/icons/hand";
    import MessageCircleIcon from "@lucide/svelte/icons/message-circle";
    import SearchIcon from "@lucide/svelte/icons/search";
    import SquareTerminalIcon from "@lucide/svelte/icons/square-terminal";
    import WrenchIcon from "@lucide/svelte/icons/wrench";
    import type { Frame, Role, ToolKind } from "./anatomy";

    let {
        role,
        frame,
        tool = null,
        live,
    }: {
        role: Role;
        frame: Frame | null;
        /** The tool's icon family for the badge of an ask or act avatar; null is the generic icon. */
        tool?: ToolKind | null;
        /** The part is in progress: the avatar animates, otherwise it rests. */
        live: boolean;
    } = $props();

    const TOOL_ICONS = {
        terminal: SquareTerminalIcon,
        edit: FilePenIcon,
        read: FileTextIcon,
        find: SearchIcon,
    };
    const ToolIcon = $derived(tool ? TOOL_ICONS[tool] : WrenchIcon);
    const dots = $derived(role === "brain-think" || frame === "compose");
    const Disc = $derived(role === "arms-act" ? HandIcon : BrainIcon);
</script>

<span
    class={[
        "avatar",
        role === "arms-act" ? "bg-arms text-arms-foreground" : "bg-brain text-brain-foreground",
        { live, dim: frame === "unrun" },
    ]}
    data-agent-avatar={role}
    data-avatar-frame={frame}
    data-avatar-live={live}
    aria-hidden="true"
>
    <Disc class="size-4" />
    <span class={["badge", { bad: frame === "failed", typing: frame === "compose", thinking: role === "brain-think" }]}>
        {#if dots}
            <i></i><i></i><i></i>
        {:else if role === "brain-reply"}
            <MessageCircleIcon class="size-2.5" />
        {:else}
            <ToolIcon class="size-2.5" />
        {/if}
    </span>
</span>

<style>
    .avatar {
        position: relative;
        display: inline-flex;
        flex: none;
        align-items: center;
        justify-content: center;
        width: 1.75rem;
        height: 1.75rem;
        border-radius: 9999px;
    }
    .avatar.dim {
        opacity: 0.55;
    }
    .badge {
        position: absolute;
        right: -0.25rem;
        bottom: -0.25rem;
        display: flex;
        align-items: center;
        justify-content: center;
        gap: 1.5px;
        min-width: 0.875rem;
        height: 0.875rem;
        padding: 0 2px;
        border-radius: 9999px;
        background: var(--card);
        color: var(--foreground);
        box-shadow: 0 0 0 2px var(--background);
    }
    .badge.bad {
        background: var(--destructive);
        color: var(--background);
    }
    .badge i {
        width: 3px;
        height: 3px;
        border-radius: 9999px;
        background: currentColor;
    }

    /* Looping keyframes escape the central reduced-motion tokens (docs/features/chat-agent-anatomy/02_dev.md, Resolved Decision 17). */
    @media (prefers-reduced-motion: no-preference) {
        .live[data-avatar-frame="act"]::after {
            content: "";
            position: absolute;
            inset: 0;
            border-radius: 9999px;
            border: 2px solid var(--arms);
            opacity: 0;
            animation: ring 1.3s ease-out infinite;
        }
        .live .thinking i {
            animation: bounce 0.9s ease-in-out infinite;
        }
        .live .typing i {
            animation: blink 1.1s ease-in-out infinite;
        }
        .live .thinking i:nth-child(2),
        .live .typing i:nth-child(2) {
            animation-delay: 0.15s;
        }
        .live .thinking i:nth-child(3),
        .live .typing i:nth-child(3) {
            animation-delay: 0.3s;
        }
        .live[data-agent-avatar="brain-reply"] .badge {
            animation: bubble 1.4s ease-in-out infinite;
        }
    }
    @keyframes ring {
        from {
            opacity: 0.7;
            scale: 1;
        }
        to {
            opacity: 0;
            scale: 1.6;
        }
    }
    @keyframes bounce {
        50% {
            translate: 0 -2px;
        }
    }
    @keyframes blink {
        50% {
            opacity: 0.25;
        }
    }
    @keyframes bubble {
        50% {
            scale: 1.18;
        }
    }
</style>
