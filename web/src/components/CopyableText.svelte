<script lang="ts">
  import { onDestroy } from "svelte";

  let { text, label }: { text: string; label: string } = $props();
  let value: HTMLElement;
  let status = $state("");
  let reset: ReturnType<typeof setTimeout> | undefined;
  onDestroy(() => clearTimeout(reset));

  async function copy() {
    clearTimeout(reset);
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      // Plain HTTP on a trusted LAN may not expose navigator.clipboard.
      const selection = window.getSelection();
      selection?.selectAllChildren(value);
      if (!selection || !document.execCommand("copy")) {
        status = "Select and copy the text manually.";
        return;
      }
      selection.removeAllRanges();
    }
    status = "Copied";
    reset = setTimeout(() => (status = ""), 2000);
  }
</script>

<div class="copyable">
  <code bind:this={value}>{text}</code>
  <button
    class="inline-action"
    type="button"
    aria-label={label}
    title={status === "Copied" ? status : label}
    onclick={copy}
  >
    <svg
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2"
      aria-hidden="true"
    >
      {#if status === "Copied"}
        <path d="m4 12 5 5L20 6" />
      {:else}
        <rect x="8" y="8" width="12" height="13" rx="2" />
        <path d="M15 8V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h3" />
      {/if}
    </svg>
  </button>
  <small role="status">{status}</small>
</div>

<style>
  .copyable {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 0.4rem;
  }
  code {
    overflow-wrap: anywhere;
    user-select: all;
  }
  button {
    display: inline-flex;
    padding: 0.35rem;
  }
</style>
