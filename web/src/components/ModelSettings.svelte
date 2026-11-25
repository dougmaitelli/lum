<script lang="ts">
  import { saveModelSettings, type Model } from "../api/admin";
  let { onsave }: { onsave: () => void } = $props();
  import Dialog from "./Dialog.svelte";
  let dialog: Dialog;
  let alias = $state(""),
    label = $state(""),
    limit = $state<number | undefined>(),
    error = $state(""),
    busy = $state(false);
  export function open(model: Model) {
    alias = model.alias;
    label = model.label;
    limit = model.context_limit ?? undefined;
    error = "";
    dialog.showModal();
  }
  async function save() {
    if (busy) return;
    busy = true;
    try {
      await saveModelSettings(alias, label.trim(), limit ?? null);
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
