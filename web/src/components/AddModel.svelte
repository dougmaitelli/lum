<script lang="ts">
  import { catalog, install, packagePresets, type Model } from "../api/admin";
  let { failures, ondownload }: { failures: Model[]; ondownload: () => void } = $props();
  let dialog: HTMLDialogElement;
  let repo = $state(""),
    preset = $state(""),
    list = $state<string[] | null>(null),
    loading = $state(false),
    failed = $state(false),
    error = $state(""),
    selected = $state(false),
    busy = $state(false);
  let hits = $derived(
    list?.filter((id) => id.toLowerCase().includes(repo.trim().toLowerCase())) ?? [],
  );
  let choices = $state<string[]>([]),
    reading = $state(false),
    lookupFailed = $state(false),
    inspected = $state(""),
    attempt = $state(0);
  let canDownload = $derived(
    !busy &&
      !reading &&
      inspected !== "" &&
      inspected === repo.trim() &&
      (!choices.length || choices.includes(preset)),
  );
  $effect(() => {
    // A retry repeats discovery for the same repository.
    void attempt;
    const input = repo.trim();
    inspected = "";
    preset = "";
    choices = [];
    lookupFailed = false;
    const lookup = /^(?:https:\/\/huggingface\.co\/)?[\w-]+\/[\w.-]+/.test(input);
    reading = lookup;
    if (!lookup) return;
    const controller = new AbortController();
    const timer = setTimeout(async () => {
      try {
        const names = await packagePresets(input, controller.signal);
        if (controller.signal.aborted) return;
        choices = names;
        preset = names.length === 1 ? names[0] : "";
        inspected = input;
      } catch {
        if (!controller.signal.aborted) lookupFailed = true;
      } finally {
        if (!controller.signal.aborted) reading = false;
      }
    }, 250);
    return () => {
      clearTimeout(timer);
      controller.abort();
    };
  });
  export async function open() {
    dialog.showModal();
    if (list || loading) return;
    loading = true;
    failed = false;
    try {
      list = await catalog();
    } catch {
      failed = true;
    } finally {
      loading = false;
    }
  }
  function resetForm() {
    repo = preset = error = "";
    selected = false;
  }
  async function submit() {
    if (!canDownload) return;
    busy = true;
    error = "";
    try {
      await install(repo, preset);
      dialog.close();
      ondownload();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<dialog bind:this={dialog} aria-label="Add model" onclose={resetForm}>
  <h2>Add model</h2>
  <p class="muted">
    Search loom-fun packages or paste a Hugging Face model link from a publisher you trust. The
    repository must contain a Loom package.
  </p>

  {#each failures as m (m.alias)}
    <details>
      <summary>Install failed: {m.alias}</summary>
      <pre class="warning">{m.error}</pre>
      <a href={"/remove/" + m.alias}>Remove failed install to retry…</a>
    </details>
  {/each}
  <div>
    <label>
      Repository
      <input
        bind:value={repo}
        oninput={() => (selected = false)}
        placeholder="Search or paste https://huggingface.co/owner/model"
        required
        autocomplete="off"
      />
    </label>
    {#if !selected}
      <small role="status">
        {loading
          ? "Loading Hugging Face catalog…"
          : failed
            ? "Catalog unavailable. Paste a repository link instead."
            : repo.startsWith("https://")
              ? "Install from repository link"
              : `${hits.length} matching packages`}
      </small>
      {#if !repo.startsWith("https://")}
        <ul id="results">
          {#each hits.slice(0, 30) as id (id)}
            <li>
              <button
                type="button"
                onclick={() => {
                  repo = id;
                  selected = true;
                }}
              >
                {id}
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    {/if}
    {#if reading}<small role="status">Loading presets…</small>
    {:else if choices.length > 1}<label>
        Preset
        <select bind:value={preset} required>
          <option value="" disabled>Select a preset</option>
          {#each choices as name (name)}<option value={name}>{name}</option>{/each}
        </select>
      </label>
    {:else if choices.length === 1}<small>Preset: {preset}</small>
    {:else if lookupFailed}<small role="status">Couldn’t load presets.</small>
      <button type="button" onclick={() => attempt++}>Retry preset lookup</button>
    {/if}
    <p class="muted">Installs the current default revision. Load the model separately.</p>
    <p class="warning" role="status">{error}</p>
    <footer>
      <button type="button" onclick={() => dialog.close()}>Close</button>
      <button type="button" class="primary" disabled={!canDownload} onclick={submit}>
        Download
      </button>
    </footer>
  </div>
</dialog>

<style>
  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
  }
</style>
