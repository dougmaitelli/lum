<script lang="ts">
  import { onMount, tick } from "svelte";
  import { reply, type Turn, type Model } from "../api/admin";
  let { model, online }: { model: Model | undefined; online: boolean } = $props();
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
    if (!model) return;
    try {
      sessionStorage.setItem("lum.draft." + model.alias, prompt);
    } catch {
      /* optional */
    }
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
    busy = true;
    prompt = "";
    turns = [...history, { role: "user", content: submitted }];
    feedback = "Waiting for reply…";
    void scroll();
    try {
      const result = await reply(alias, submitted, history);
      turns = result.turns;
      store(alias, result.turns);
      feedback = result.metrics;
      void scroll();
    } catch (e) {
      turns = history;
      prompt = submitted;
      feedback = String(e);
    } finally {
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
      <p>{turn.content}</p>
    </article>
  {/each}
</section>
{#if model}
  <form
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
      <button class="primary" disabled={!online || !model.loaded || busy}>Send</button>
    </div>
  </form>
{/if}
