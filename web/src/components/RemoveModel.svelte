<script lang="ts">
  import { modelAction, type Model } from "../api/admin";
  import Dialog from "./Dialog.svelte";
  let { onremoved }: { onremoved: (alias: string) => Promise<void> } = $props();
  let dialog: Dialog;
  let model = $state<Model>(),
    busy = $state(false),
    error = $state("");
  export function open(target: Model) {
    model = target;
    error = "";
    dialog.showModal();
  }
  async function remove() {
    if (!model || busy) return;
    const alias = model.alias;
    busy = true;
    error = "";
    try {
      await modelAction(alias, "delete");
      await onremoved(alias);
      dialog.close();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }
</script>

<Dialog
  bind:this={dialog}
  title={`Remove ${model?.label || model?.alias || "model"}?`}
  closeLabel="Cancel"
  locked={busy}
>
  <p>
    This deletes the local installation of <strong>{model?.repo}</strong>
    and its library entry.
  </p>
  {#if error}<p class="warning" role="alert">{error}</p>{/if}
  {#snippet actions()}<button type="button" disabled={busy} onclick={remove}>
      Remove files
    </button>{/snippet}
</Dialog>
