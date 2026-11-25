<script lang="ts">
  import { onMount } from "svelte";
  import { modelAction, snapshot, type Model } from "../api/admin";
  let { alias }: { alias: string } = $props();
  let model = $state<Model>(),
    busy = $state(false),
    error = $state("");
  onMount(() => {
    void snapshot()
      .then((s) => {
        model = s.models.find((m) => m.alias === alias);
        if (!model) error = "Unknown model alias.";
      })
      .catch((e) => (error = String(e)));
  });
  async function remove() {
    busy = true;
    error = "";
    try {
      await modelAction(alias, "delete");
      location.replace("/");
    } catch (e) {
      error = String(e);
      busy = false;
    }
  }
</script>

<svelte:head><title>Remove model · Lum</title></svelte:head>
<main class="notice">
  <h1>Remove {alias}?</h1>
  {#if error}<p class="warning" role="alert">{error}</p>{/if}
  {#if model}<p>This deletes the local installation of {model.repo} and its library entry.</p>{/if}
  <div class="compose-tools">
    <button disabled={busy || !model} onclick={remove}>Remove files</button>
    <a href="/">Cancel</a>
  </div>
</main>
