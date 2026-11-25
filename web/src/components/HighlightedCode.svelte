<script lang="ts">
  import hljs from "highlight.js/lib/common";
  import "highlight.js/styles/github-dark-dimmed.css";
  let { text, language, tail = "" }: { text: string; language: string; tail?: string } = $props();
  // Never guess a grammar for unlabelled snippets, or run a grammar over an
  // unbounded model response. The fallback is ordinary Svelte-escaped text.
  let highlighted = $derived(
    language && text.length <= 64 * 1024 && hljs.getLanguage(language)
      ? hljs.highlight(text, { language, ignoreIllegals: true }).value
      : null,
  );
</script>

<!-- eslint-disable-next-line svelte/no-at-html-tags -- Highlight.js escapes source text and emits token spans. -->
<pre><code>{#if highlighted !== null}{@html highlighted}{:else}{text}{/if}{tail}</code></pre>

<style>
  pre {
    margin: 0;
    padding: 8px 10px;
    overflow-x: auto;
    white-space: pre;
    tab-size: 4;
  }
</style>
