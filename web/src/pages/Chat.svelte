<script lang="ts">
  import { onMount } from "svelte";
  import { snapshot, modelAction, type Snapshot } from "../api/admin";
  import { forgetChat, reconcileChats } from "../api/chat-storage";
  import SignOut from "../components/SignOut.svelte";
  let { oidc }: { oidc: boolean } = $props();
  import Conversation from "../components/Conversation.svelte";
  import StatusBar from "../components/StatusBar.svelte";
  import ModelSettings from "../components/ModelSettings.svelte";
  import Downloads from "../components/Downloads.svelte";
  import ErrorDialog from "../components/ErrorDialog.svelte";
  import RemoveModel from "../components/RemoveModel.svelte";
  import AddModel from "../components/AddModel.svelte";
  let data = $state<Snapshot>({
      models: [],
      online: false,
      mock: false,
      vram: [],
      progress: { stage: "", percent: null },
    }),
    selected = $state(location.pathname.split("/chat/")[1] || ""),
    stale = $state(false),
    working = $state(false);
  let add: AddModel, downloads: Downloads, settings: ModelSettings;
  let conversation = $state<Conversation>();
  let models = $derived(data.models.filter((m) => m.state === "installed")),
    current = $derived(models.find((m) => m.alias === selected)),
    jobs = $derived(
      data.models
        .filter((m) => m.state === "downloading" || m.state === "queued")
        .sort((a, b) => a.state.localeCompare(b.state)),
    );
  let errors: ErrorDialog, removal: RemoveModel;
  let failedJobs: Set<string> | undefined;
  let newest = 0,
    lastModelError = "";
  async function refresh() {
    const id = ++newest;
    try {
      const result = await snapshot();
      if (id !== newest) return;
      reconcileChats(result.models);
      const failures = result.models.filter((m) => m.state === "failed");
      for (const job of failures) {
        if (failedJobs && !failedJobs.has(job.installation))
          errors.show(`Download failed: ${job.alias}`, job.error);
      }
      failedJobs = new Set(failures.map((m) => m.installation));
      data = result;
      stale = false;
      if (!models.some((m) => m.alias === selected))
        selected = (models.find((m) => m.loaded) ?? models[0])?.alias ?? "";
      const model = models.find((m) => m.alias === selected);
      const key = model?.error ? `${model.alias}:${model.error}` : "";
      if (model && key && key !== lastModelError)
        errors.show(`Model error: ${model.label || model.alias}`, model.error);
      lastModelError = key;
    } catch {
      if (id === newest) stale = true;
    }
  }
  async function change(op: string, alias = current?.alias) {
    if (!alias || working) return;
    working = true;
    try {
      await modelAction(alias, op);
      await refresh();
    } catch (e) {
      errors.show(
        `Could not ${op === "start" ? "load" : op === "stop" ? "unload" : op} ${alias}`,
        e,
      );
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
        <div class="model-row">
          <a
            class="model"
            title={m.alias}
            href={"/chat/" + m.alias}
            aria-current={m.alias === selected ? "page" : undefined}
          >
            <span>{m.label || m.alias}</span>

            {#if m.loaded && !stale}
              <span class="loaded" role="img" aria-label="Loaded" title="Loaded"></span>
            {/if}
          </a>
          <button
            class="inline-action row-actions pin"
            aria-label={`Pin ${m.label || m.alias}`}
            aria-pressed={m.pinned}
            title={m.pinned ? "Unpin model" : "Pin model at startup"}
            disabled={working || stale}
            onclick={() => change(m.pinned ? "unpin" : "pin", m.alias)}
          >
            <svg
              width="16"
              height="16"
              viewBox="0 0 24 24"
              stroke="currentColor"
              stroke-width="1.6"
              fill={m.pinned ? "currentColor" : "none"}
              aria-hidden="true"
            >
              <path d="M8 3h8l-1 7 3 3v2H6v-2l3-3zM12 15v6" />
            </svg>
          </button>
          {#if m.registered || m.desired}
            <button
              class="inline-action row-actions"
              aria-label={`Unload ${m.alias}`}
              title={`Unload ${m.alias}`}
              disabled={working || stale || !data.online}
              onclick={() => change("stop", m.alias)}
            >
              <svg
                width="16"
                height="16"
                viewBox="0 0 24 24"
                fill="currentColor"
                aria-hidden="true"
              >
                <path d="m12 4 9 12H3zM3 18h18v3H3z" />
              </svg>
            </button>
          {:else}
            <button
              class="inline-action row-actions"
              disabled={working || stale || !data.online}
              aria-label={`Options for ${m.label || m.alias}`}
              title="Model options"
              popovertarget={`options-${m.alias}`}
              onclick={(e) => {
                const rect = e.currentTarget.getBoundingClientRect();
                const menu = document.getElementById(`options-${m.alias}`);
                menu?.style.setProperty("--menu-right", `${window.innerWidth - rect.right}px`);
                menu?.style.setProperty("--menu-top", `${rect.bottom + 3}px`);
              }}
            >
              …
            </button>
            <div popover class="model-options" id={`options-${m.alias}`}>
              <button
                class="menu-action"
                onclick={() => {
                  document.getElementById(`options-${m.alias}`)?.hidePopover();
                  settings.open(m);
                }}
              >
                Settings
              </button>
              <button
                class="menu-action remove-action"
                onclick={() => {
                  document.getElementById(`options-${m.alias}`)?.hidePopover();
                  removal.open(m);
                }}
              >
                Remove
              </button>
            </div>
          {/if}
        </div>
      {/each}
    </nav>
    <div class="side-foot">
      <a class="button-link" href="/api">API keys</a>
      {#if oidc}<SignOut />{/if}
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
        <h1>{current?.label || current?.alias || "New conversation"}</h1>

        {#if current}
          <small>
            {[current.repo, current.preset, current.revision.slice(0, 12)]
              .filter(Boolean)
              .join(" · ")}
          </small>
        {/if}
      </div>
      <div id="controls">
        {#if current}
          <button disabled={!conversation?.canClear()} onclick={() => conversation?.clear()}>
            Clear chat
          </button>
        {/if}
      </div>
    </header>

    {#key current?.installation}
      <Conversation
        bind:this={conversation}
        model={current}
        online={data.online && !stale}
        {working}
        onloadmodel={() => change("start")}
        onfailure={(e) => errors.show("Chat error", e)}
      />
    {/key}
  </main>
  <StatusBar {data} {stale} />
  <ErrorDialog bind:this={errors} />
  <RemoveModel
    bind:this={removal}
    onremoved={async (alias) => {
      forgetChat(alias);
      if (location.pathname === `/chat/${alias}`) history.replaceState(null, "", "/");
      await refresh();
    }}
  />
  <ModelSettings bind:this={settings} onsave={refresh} />
  <Downloads bind:this={downloads} {jobs} progress={data.progress} />
  <AddModel
    bind:this={add}
    onremove={(model) => removal.open(model)}
    failures={data.models.filter(
      (m) => m.state === "failed" || (m.state === "deleting" && m.error),
    )}
    ondownload={() => {
      downloads.open();
      void refresh();
    }}
  />
</div>
