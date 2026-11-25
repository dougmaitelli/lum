<script lang="ts">
  import type { Snippet } from "svelte";
  let {
    title,
    children,
    actions,
    onclose,
    locked = false,
    closeLabel = "Close",
  }: {
    title: string;
    children: Snippet;
    actions?: Snippet;
    closeLabel?: string;
    onclose?: () => void;
    locked?: boolean;
  } = $props();
  let element: HTMLDialogElement;
  export function showModal() {
    element.showModal();
  }
  export function close() {
    element.close();
  }
</script>

<dialog
  bind:this={element}
  aria-label={title}
  {onclose}
  oncancel={(e) => {
    if (locked) e.preventDefault();
  }}
>
  <h2>{title}</h2>
  {@render children()}
  <footer>
    <button type="button" disabled={locked} onclick={close}>{closeLabel}</button>
    {@render actions?.()}
  </footer>
</dialog>

<style>
  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    margin-top: 12px;
  }
</style>
