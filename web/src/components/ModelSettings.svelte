<script lang="ts">
  import { saveModelSettings, type Model } from "../api/admin";
  let { onsave }: { onsave: () => void } = $props();
  import Dialog from "./Dialog.svelte";
  let dialog: Dialog;
  let alias = $state(""),
    label = $state(""),
    limit = $state<number | undefined>(),
    disableThinking = $state(false),
    error = $state(""),
    busy = $state(false);
  export function open(model: Model) {
    alias = model.alias;
    label = model.label;
    limit = model.context_limit ?? undefined;
    disableThinking = model.disable_thinking;
    error = "";
    dialog.showModal();
  }
  async function save() {
    if (busy) return;
    busy = true;
    try {
      await saveModelSettings(alias, label.trim(), limit ?? null, disableThinking);
      onsave();
      dialog.close();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<Dialog bind:this={dialog} title="Model settings" locked={busy}>
  <p class="muted">
    API model ID: <code>{alias}</code>
  </p>
  <form
    id="model-settings-form"
    onsubmit={(e) => {
      e.preventDefault();
      void save();
    }}
  >
    <label>
      Display label
      <input bind:value={label} maxlength="64" placeholder={alias} />
    </label>
    <label>
      Context limit (tokens)
      <input
        type="number"
        min="1"
        max="2147483647"
        step="1"
        bind:value={limit}
        placeholder="Model default"
      />
    </label>
    <p class="muted">
      Includes the prompt, chat history, and reply. Leave blank to use the model preset. Applies on
      the next load.
    </p>
    <label class="thinking">
      <input type="checkbox" bind:checked={disableThinking} />
      Disable thinking
    </label>
    <p class="muted">
      Request direct answers in chat and API clients. Leave unchecked for the preset default; not
      all packages support this option.
    </p>
    {#if error}<p role="alert" class="warning">{error}</p>{/if}
  </form>
  {#snippet actions()}<button
      class="primary"
      type="submit"
      form="model-settings-form"
      disabled={busy}
    >
      Save
    </button>{/snippet}
</Dialog>

<style>
  .thinking {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  .thinking input {
    width: auto;
    min-height: 0;
  }
</style>
