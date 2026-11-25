<script lang="ts">
  import { onMount } from "svelte";
  import { snapshot, modelAction, type Snapshot } from "../api/admin";
  import Conversation from "../components/Conversation.svelte";
  import StatusBar from "../components/StatusBar.svelte";
  import Downloads from "../components/Downloads.svelte";
  import AddModel from "../components/AddModel.svelte";
  let data = $state<Snapshot>({
      models: [],
      online: false,
      mock: false,
      vram: [],
      progress: { stage: "", percent: null },
    }),
    selected = $state(location.pathname.split("/chat/")[1] || ""),
    error = $state(""),
    stale = $state(false),
    working = $state(false);
  let add: AddModel, downloads: Downloads;
  let models = $derived(data.models.filter((m) => m.state === "installed")),
    current = $derived(models.find((m) => m.alias === selected)),
    jobs = $derived(
      data.models
        .filter((m) => m.state === "downloading" || m.state === "queued")
        .sort((a, b) => a.state.localeCompare(b.state)),
    );
  let newest = 0;
  async function refresh() {
    const id = ++newest;
    try {
      const result = await snapshot();
      if (id !== newest) return;
      data = result;
      stale = false;
      if (!models.some((m) => m.alias === selected))
        selected = (models.find((m) => m.loaded) ?? models[0])?.alias ?? "";
    } catch {
      if (id === newest) stale = true;
    }
  }
  async function change(op: string) {
    if (!current) return;
    working = true;
    error = "";
    try {
      await modelAction(current.alias, op);
      await refresh();
    } catch (e) {
      error = String(e);
    } finally {
      working = false;
    }
  }
  onMount(() => {
    let timer: ReturnType<typeof setTimeout>,
      stopped = false;
    async function poll() {
      if (!document.hidden) await refresh();
      if (!stopped) timer = setTimeout(poll, downloads?.isOpen() ? 700 : 3000);
    }
    void poll();
    return () => {
      stopped = true;
      clearTimeout(timer);
    };
  });
</script>

<div class="shell">
  <aside>
    <header>
      <a class="brand" href="/">lum</a>

      {#if data.mock}
        <small class="preview">Mock runtime</small>
      {/if}
    </header>
    <div class="side-heading">
      <h2>Models</h2>
      <button onclick={() => add.open()}>+ Add model</button>
    </div>
    <nav id="models" aria-label="Downloaded models">
      {#if !models.length}
        <p class="muted">No models yet.</p>
      {/if}
      {#each models as m (m.alias)}
        <a
          class="model"
          href={"/chat/" + m.alias}
          aria-current={m.alias === selected ? "page" : undefined}
        >
          <span>{m.alias}</span>

          {#if m.loaded && !stale}
            <span class="loaded" role="img" aria-label="Loaded" title="Loaded"></span>
          {/if}
        </a>
      {/each}
    </nav>
    <div class="side-foot">
      <a class="button-link" href="/api">API keys</a>
      <button
        class="downloads-trigger"
        popovertarget="downloads"
        aria-label={`Downloads: ${jobs.length} queued or active`}
        title="Downloads"
        onclick={refresh}
      >
        <svg
          width="16"
          height="16"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
          aria-hidden="true"
        >
          <path d="M12 3v12m-5-5 5 5 5-5M4 16v4h16v-4" />
        </svg>
        <span class="downloads-count">{jobs.length}</span>
      </button>
    </div>
  </aside>
  <main class="chat">
    <header class="chat-head">
      <div>
        <small>Chat</small>
        <h1>{current?.alias ?? "New conversation"}</h1>

        {#if current}
          <small>
            {[current.repo, current.preset, current.revision.slice(0, 12)]
              .filter(Boolean)
              .join(" · ")}
          </small>
        {/if}
      </div>
      <div id="controls">
        {#if current && data.online && !stale}
          {#if current.registered || current.desired}
            <button disabled={working} onclick={() => change("stop")}>Unload</button>
          {:else}
            <button disabled={working} onclick={() => change("start")}>Load model</button>
            <a class="quiet" href={"/remove/" + current.alias}>Remove…</a>
          {/if}
        {/if}
        {#if error || current?.error}
          <span class="warning">
            {error || current?.error}
          </span>
        {/if}
      </div>
    </header>

    {#key current?.alias}
      <Conversation model={current} online={data.online && !stale} />
    {/key}
  </main>
  <StatusBar {data} {stale} />
  <Downloads bind:this={downloads} {jobs} progress={data.progress} />
  <AddModel
    bind:this={add}
    failures={data.models.filter((m) => m.state === "failed")}
    ondownload={() => {
      downloads.open();
      void refresh();
    }}
  />
</div>
