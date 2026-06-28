/** Svelte action: moves the node to document.body so it escapes any overflow:hidden ancestor. */
export function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return { destroy() { node.remove(); } };
}
