<script lang="ts">
  import { segments } from "./message-format";
  import HighlightedCode from "./HighlightedCode.svelte";
  let { content }: { content: string } = $props();
  let parts = $derived(segments(content));
</script>

<div class="message-body">
  {#each parts as part, index (index)}
    {#if part.kind === "code"}
      <div class="code-block">
        {#if part.language}<small>{part.language}</small>{/if}
        <HighlightedCode text={part.value} language={part.language} />
      </div>
    {:else}
      <p>{part.value}</p>
    {/if}
  {/each}
</div>

<style>
  p {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .code-block {
    background: var(--bg);
    border: 1px solid var(--line);
    border-radius: 5px;
  }
  .message-body > :global(* + *) {
    margin-top: 8px;
  }
  small {
    display: block;
    padding: 3px 10px;
    border-bottom: 1px solid var(--line);
    overflow-wrap: anywhere;
  }
</style>
