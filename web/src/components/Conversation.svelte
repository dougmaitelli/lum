<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
  import type { Turn, Model } from "../api/admin";
  import MessageContent from "./MessageContent.svelte";
  import { streamReply } from "../api/chat";
  let {
    model,
    online,
    working,
    onloadmodel,
  }: { model: Model | undefined; online: boolean; working: boolean; onloadmodel: () => void } =
    $props();
  function previous(): Turn[] {
    try {
      return JSON.parse(sessionStorage.getItem("lum.chat." + model?.alias) ?? "[]");
    } catch {
      return [];
    }
  }
  function previousDraft() {
    try {
      return sessionStorage.getItem("lum.draft." + model?.alias) ?? "";
    } catch {
      return "";
    }
  }
  let turns = $state<Turn[]>(previous()),
    prompt = $state(previousDraft()),
    feedback = $state(""),
    busy = $state(false),
    messages: HTMLElement;
  $effect(() => {
    if (!model || busy) return;
    try {
      sessionStorage.setItem("lum.draft." + model.alias, prompt);
    } catch {
      /* optional */
    }
  });
  let disposed = false,
    active: AbortController | undefined;
  onDestroy(() => {
    disposed = true;
    active?.abort();
  });
  onMount(() => {
    void scroll();
  });
  async function scroll() {
    await tick();
    messages?.scrollTo(0, messages.scrollHeight);
  }
  function store(alias: string, value: Turn[]) {
    try {
      sessionStorage.setItem("lum.chat." + alias, JSON.stringify(value));
    } catch {
      /* storage optional */
    }
  }
  async function send() {
    if (!model?.loaded || !online || busy || !prompt.trim()) return;
    const alias = model.alias,
      submitted = prompt,
      history = $state.snapshot(turns);
    // Keep an unsent copy until the response is committed. Reload restores it
    // as a draft, never as a new request.
    try {
      sessionStorage.setItem("lum.draft." + alias, submitted);
    } catch {
      /* optional */
    }
    busy = true;
    prompt = "";
    turns = [...history, { role: "user", content: submitted }];
    feedback = "Waiting for reply…";
    void scroll();
    const controller = new AbortController(),
      started = performance.now();
    active = controller;
    let answer = "";
    try {
      const result = await streamReply(alias, submitted, history, controller.signal, (text) => {
        if (disposed || controller.signal.aborted) return;
        answer += text;
        if (!answer.trimStart()) return;
        if (turns.length === history.length + 1) turns.push({ role: "assistant", content: answer });
        else turns[turns.length - 1].content = answer;
        feedback = "Generating…";
        void scroll();
      });
      if (disposed) return;
      if (!answer.trim())
        throw Error(
          result.reason === "length"
            ? "Loom reached its generation limit before returning answer text."
            : "Loom returned no answer text.",
        );
      const sec = (performance.now() - started) / 1000;
      const usage =
        result.tokens === null
          ? "Token count not reported"
          : `${result.tokens} output tokens · ${(result.tokens / sec).toFixed(1)} output tok/s`;
      feedback = `${result.reason === "length" ? "Stopped at the model’s output limit · " : ""}${sec.toFixed(2)} s · ${usage}`;
      store(alias, turns);
    } catch (e) {
      if (disposed) return;
      if (answer.trim()) store(alias, turns);
      else {
        turns = history;
        prompt = submitted;
      }
      feedback = controller.signal.aborted ? "Generation stopped." : String(e);
    } finally {
      active = undefined;
      busy = false;
    }
  }
  export function canClear() {
    return !busy && Boolean(turns.length || prompt || feedback);
  }
  export function clear() {
    if (busy) return;
    turns = [];
    prompt = "";
    feedback = "";
    if (model) store(model.alias, []);
  }
</script>

<section id="messages" aria-label="Conversation" bind:this={messages}>
  {#if !turns.length}
    <div class="intro">
      <h2>Start a conversation</h2>
      <p class="muted">
        {model ? "Send a message once the model is loaded." : "Add a model to start chatting."}
      </p>
    </div>
  {/if}

  {#each turns as turn, i (i)}
    <article class="message" data-role={turn.role}>
      <strong>{turn.role === "user" ? "You" : "Response"}</strong>
      <MessageContent content={turn.content} />
    </article>
  {/each}
</section>
{#if model && !model.loaded && !busy}
  <button
    class="primary composer-area load-model"
    disabled={!online || working || model.registered || model.desired}
    onclick={onloadmodel}
  >
    {!online
      ? "Engine unavailable"
      : model.registered || model.desired
        ? model.error
          ? "Model unavailable"
          : model.desired
            ? "Loading model…"
            : "Unloading model…"
        : "Load model"}
  </button>
{:else if model}
  <form
    class="composer-area"
    id="composer"
    onsubmit={(e) => {
      e.preventDefault();
      void send();
    }}
  >
    <label class="sr" for="prompt">Message</label>
    <textarea
      id="prompt"
      bind:value={prompt}
      placeholder="Write a message…"
      rows="2"
      maxlength="8192"
      readonly={busy}
      required
      onkeydown={(e) => {
        if (e.key === "Enter" && !e.shiftKey && !e.isComposing) {
          e.preventDefault();
          e.currentTarget.form?.requestSubmit();
        }
      }}
    ></textarea>
    <div class="compose-tools">
      <small id="feedback" role="status">{feedback}</small>
      {#if busy}
        <button type="button" onclick={() => active?.abort()}>Stop</button>
      {:else}
        <button class="primary" disabled={!online || !model.loaded}>Send</button>
      {/if}
    </div>
  </form>
{/if}
