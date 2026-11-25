<script lang="ts">
  import { segments } from "./message-format";
  import HighlightedCode from "./HighlightedCode.svelte";
  import { untrack } from "svelte";
  let { content, streaming = false }: { content: string; streaming?: boolean } = $props();
  let checkpoint = $state("");
  $effect(() => {
    if (!streaming) return;
    checkpoint = untrack(() => content);
    const timer = setInterval(() => {
      checkpoint = content;
    }, 150);
    return () => clearInterval(timer);
  });
  let prefix = $derived(streaming ? checkpoint : content);
  let parts = $derived(segments(prefix));
  let last = $derived(parts.at(-1));
  let tail = $derived(streaming ? content.slice(prefix.length) : "");
</script>

<div class="message-body">
  {#each parts as part, index (index)}
    {#if part.kind === "code"}
      <div class="code-block">
        {#if part.language}<small>{part.language}</small>{/if}
        <HighlightedCode
          text={part.value}
          language={part.language}
          tail={part.open && index === parts.length - 1 ? tail : ""}
        />
      </div>
    {:else}
      <p>{part.value}{index === parts.length - 1 ? tail : ""}</p>
    {/if}
  {/each}
  {#if tail && last?.kind === "code" && !last.open}<p>{tail}</p>{/if}
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
